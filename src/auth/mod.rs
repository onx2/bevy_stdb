//! Authentication types and runtime state for SpacetimeDB OIDC flows.

#[cfg(all(feature = "browser", target_arch = "wasm32"))]
#[path = "web.rs"]
mod auth_imp;

#[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
#[path = "native.rs"]
mod auth_imp;

#[cfg(all(not(feature = "browser"), target_arch = "wasm32"))]
compile_error!("wasm32 builds require the `browser` feature");

use crate::{
    connection::{StdbConnection, StdbConnectionController},
    message::StdbConnectedMessage,
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Message, MessageReader, Res, ResMut, Resource};
use bevy_state::prelude::{AppExtStates, NextState, States};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::marker::PhantomData;

/// Configures OIDC authentication for a SpacetimeDB connection.
#[derive(Clone, Debug)]
pub struct StdbAuthOptions {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The authorization endpoint.
    pub auth_endpoint: String,
    /// The token endpoint.
    pub token_endpoint: String,
    /// The redirect URI used by the client.
    pub redirect_uri: String,
    /// The requested scopes.
    pub scopes: Vec<String>,
    /// The startup behavior used for the first auth attempt.
    pub startup_behavior: StdbAuthStartupBehavior,
    /// The token persistence policy.
    pub storage: StdbTokenStorage,
}

impl Default for StdbAuthOptions {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            auth_endpoint: String::new(),
            token_endpoint: String::new(),
            redirect_uri: String::new(),
            scopes: Vec::new(),
            startup_behavior: StdbAuthStartupBehavior::default(),
            storage: StdbTokenStorage::default(),
        }
    }
}

/// Defines how authentication starts when auth is enabled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StdbAuthStartupBehavior {
    /// Attempts silent authentication before interactive login.
    #[default]
    SilentFirst,
    /// Requires interactive login before connecting.
    Interactive,
}

/// Defines how auth tokens are persisted across launches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StdbTokenStorage {
    /// Disables token persistence.
    #[default]
    None,
    /// Uses the target platform's preferred persistence strategy.
    PlatformDefault,
}

/// Stores the token payload returned by the token endpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenResponse {
    /// The access token used for SpacetimeDB connections.
    pub access_token: String,
    /// The number of seconds before the access token expires.
    pub expires_in: u64,
    /// The optional refresh token.
    pub refresh_token: Option<String>,
    /// The granted scopes.
    pub scope: Option<String>,
    /// The token type.
    pub token_type: String,
    /// The optional ID token.
    pub id_token: Option<String>,
}

/// Requests an interactive login attempt.
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct RequestLoginMessage;

/// Requests logout for the current auth session.
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct RequestLogoutMessage;

/// Reports a successful auth result.
#[derive(Message, Clone, Debug)]
pub struct AuthSuccessMessage(pub TokenResponse);

/// Reports an auth failure.
#[derive(Message, Clone, Debug)]
pub struct AuthFailureMessage {
    /// The failure description.
    pub message: String,
}

/// Tracks the current auth lifecycle.
#[derive(States, Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum StdbAuthState {
    /// No authenticated session is available.
    #[default]
    Unauthenticated,
    /// An auth attempt is in progress.
    Authenticating,
    /// A token is available for connection attempts.
    Authenticated,
}

/// Stores the active auth tokens.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct StdbCurrentTokens {
    /// The current access token.
    pub access_token: Option<String>,
    /// The current refresh token.
    pub refresh_token: Option<String>,
    /// The current access token lifetime, in seconds.
    pub expires_in: Option<u64>,
    /// The current granted scopes.
    pub scope: Option<String>,
    /// The current token type.
    pub token_type: Option<String>,
    /// The current ID token.
    pub id_token: Option<String>,
}

impl StdbCurrentTokens {
    /// Returns the current access token, if present.
    pub fn access_token(&self) -> Option<&str> {
        self.access_token.as_deref()
    }

    /// Returns `true` when an access token is available.
    pub fn has_access_token(&self) -> bool {
        self.access_token.is_some()
    }

    /// Clears the stored tokens.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn replace_from_response(&mut self, response: &TokenResponse) {
        self.access_token = Some(response.access_token.clone());
        self.refresh_token = response.refresh_token.clone();
        self.expires_in = Some(response.expires_in);
        self.scope = response.scope.clone();
        self.token_type = Some(response.token_type.clone());
        self.id_token = response.id_token.clone();
    }

    pub(crate) fn set_access_token(&mut self, token: impl Into<String>) {
        self.access_token = Some(token.into());
    }
}

/// Stores the configured auth options.
#[derive(Resource, Clone, Debug)]
pub(crate) struct StdbAuthConfig {
    /// The configured auth options.
    pub options: StdbAuthOptions,
}

/// Stores internal auth runtime state.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StdbAuthRuntime {
    /// Indicates whether new token-less connection attempts are blocked.
    pub connect_blocked: bool,
    /// Indicates whether a connection should resume after auth succeeds.
    pub pending_connect: bool,
}

/// Installs shared auth resources and systems.
pub(crate) struct StdbAuthPlugin<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    options: StdbAuthOptions,
    initial_access_token: Option<String>,
    _marker: PhantomData<(C, M)>,
}

impl<C: DbConnection<Module = M> + DbContext + Send + Sync, M: SpacetimeModule<DbConnection = C>>
    StdbAuthPlugin<C, M>
{
    /// Creates a new [`StdbAuthPlugin`].
    pub(crate) fn new(options: StdbAuthOptions, initial_access_token: Option<String>) -> Self {
        Self {
            options,
            initial_access_token,
            _marker: PhantomData,
        }
    }
}

impl<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
> Plugin for StdbAuthPlugin<C, M>
{
    fn build(&self, app: &mut App) {
        app.init_state::<StdbAuthState>();

        app.add_message::<RequestLoginMessage>();
        app.add_message::<RequestLogoutMessage>();
        app.add_message::<AuthSuccessMessage>();
        app.add_message::<AuthFailureMessage>();

        let mut tokens = StdbCurrentTokens::default();
        if let Some(token) = self.initial_access_token.clone() {
            tokens.set_access_token(token);
        }

        app.insert_resource(StdbAuthConfig {
            options: self.options.clone(),
        });
        app.insert_resource(tokens);
        app.init_resource::<StdbAuthRuntime>();

        app.add_systems(
            PreUpdate,
            (
                handle_login_requests,
                handle_logout_requests::<C>,
                handle_auth_failure_messages,
                handle_auth_success_messages,
                sync_connected_token,
            )
                .chain()
                .in_set(StdbSet::StateSync),
        );
    }
}

/// Updates auth state from login requests.
fn handle_login_requests(
    mut login_requests: MessageReader<RequestLoginMessage>,
    mut runtime: ResMut<StdbAuthRuntime>,
    mut next_auth_state: ResMut<NextState<StdbAuthState>>,
) {
    if login_requests.read().next().is_none() {
        return;
    }

    runtime.connect_blocked = false;
    next_auth_state.set(StdbAuthState::Authenticating);
}

/// Updates auth state from logout requests.
fn handle_logout_requests<C: DbContext + Send + Sync + 'static>(
    mut logout_requests: MessageReader<RequestLogoutMessage>,
    mut runtime: ResMut<StdbAuthRuntime>,
    mut tokens: ResMut<StdbCurrentTokens>,
    mut next_auth_state: ResMut<NextState<StdbAuthState>>,
    active_connection: Option<Res<StdbConnection<C>>>,
) {
    if logout_requests.read().next().is_none() {
        return;
    }

    runtime.connect_blocked = true;
    runtime.pending_connect = false;
    tokens.clear();

    if let Some(connection) = active_connection {
        let _ = connection.disconnect();
    }

    next_auth_state.set(StdbAuthState::Unauthenticated);
}

/// Updates auth state from auth failures.
fn handle_auth_failure_messages(
    mut failures: MessageReader<AuthFailureMessage>,
    mut runtime: ResMut<StdbAuthRuntime>,
    mut next_auth_state: ResMut<NextState<StdbAuthState>>,
) {
    if failures.read().next().is_none() {
        return;
    }

    runtime.pending_connect = false;
    next_auth_state.set(StdbAuthState::Unauthenticated);
}

/// Updates auth state from successful auth results.
fn handle_auth_success_messages(
    mut successes: MessageReader<AuthSuccessMessage>,
    mut runtime: ResMut<StdbAuthRuntime>,
    mut tokens: ResMut<StdbCurrentTokens>,
    mut next_auth_state: ResMut<NextState<StdbAuthState>>,
    mut controller: Option<ResMut<StdbConnectionController>>,
) {
    let Some(response) = successes.read().last().map(|msg| msg.0.clone()) else {
        return;
    };

    let access_token = response.access_token.clone();

    tokens.replace_from_response(&response);
    runtime.connect_blocked = false;
    next_auth_state.set(StdbAuthState::Authenticated);

    if runtime.pending_connect {
        runtime.pending_connect = false;

        if let Some(controller) = controller.as_mut() {
            controller.connect_with_token(access_token);
        }
    }
}

/// Updates the stored access token from successful connections.
fn sync_connected_token(
    mut connected: MessageReader<StdbConnectedMessage>,
    mut runtime: ResMut<StdbAuthRuntime>,
    mut tokens: ResMut<StdbCurrentTokens>,
    mut next_auth_state: ResMut<NextState<StdbAuthState>>,
) {
    let Some(access_token) = connected.read().last().map(|msg| msg.access_token.clone()) else {
        return;
    };

    runtime.connect_blocked = false;
    runtime.pending_connect = false;
    tokens.set_access_token(access_token);
    next_auth_state.set(StdbAuthState::Authenticated);
}

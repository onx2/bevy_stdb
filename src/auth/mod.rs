#![allow(dead_code)]
//! Authentication types and shared runtime state for SpacetimeDB OIDC flows.

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
    message::{
        AuthFailureMessage, AuthSuccessMessage, RequestLoginMessage, RequestLogoutMessage,
        StdbConnectedMessage,
    },
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{
    Commands, IntoScheduleConfigs, MessageReader, MessageWriter, Res, ResMut, Resource,
};
#[cfg(all(feature = "browser", target_arch = "wasm32"))]
use bevy_ecs::system::SystemState;
#[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
use crossbeam_channel::{Receiver, TryRecvError, bounded};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::marker::PhantomData;

#[cfg(all(feature = "browser", target_arch = "wasm32"))]
type PendingAuthResult = Option<Result<TokenResponse, String>>;

#[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
type PendingAuthResult = Result<TokenResponse, String>;

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
    Silent,
    /// Requires interactive login before connecting.
    Interactive,
}

/// Defines how auth tokens are persisted across launches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StdbTokenStorage {
    /// Disables token persistence.
    None,
    /// Uses the target platform's preferred persistence strategy.
    #[default]
    PlatformDefault,
}

/// Stores the token payload returned by the token endpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// Stores the configured auth options.
#[derive(Resource, Clone, Debug)]
pub(crate) struct StdbAuthConfig {
    /// The configured auth options.
    pub options: StdbAuthOptions,
}

/// Stores the current in-memory auth token bundle.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct CurrentAuthTokens(pub Option<TokenResponse>);

impl CurrentAuthTokens {
    /// Returns the current access token, if present.
    pub fn access_token(&self) -> Option<&str> {
        self.0.as_ref().map(|tokens| tokens.access_token.as_str())
    }

    /// Returns the current refresh token, if present.
    pub fn refresh_token(&self) -> Option<&str> {
        self.0
            .as_ref()
            .and_then(|tokens| tokens.refresh_token.as_deref())
    }

    /// Replaces the current token bundle.
    pub fn replace(&mut self, tokens: TokenResponse) {
        self.0 = Some(tokens);
    }

    /// Clears the current token bundle.
    pub fn clear(&mut self) {
        self.0 = None;
    }

    /// Updates only the current access token while preserving other token metadata.
    pub(crate) fn set_access_token(&mut self, token: impl Into<String>) {
        let token = token.into();

        match self.0.as_mut() {
            Some(current) => current.access_token = token,
            None => {
                self.0 = Some(TokenResponse {
                    access_token: token,
                    ..TokenResponse::default()
                });
            }
        }
    }
}

/// Describes how a pending auth attempt was initiated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PendingAuthMode {
    /// Startup or manual connection gating should prefer silent auth first.
    #[default]
    Silent,
    /// Explicit interactive login should begin browser auth immediately.
    Interactive,
    /// Reconnect-triggered auth should remain silent.
    Reconnect,
}

#[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
pub(crate) enum PendingAuthStatus {
    Pending(Receiver<PendingAuthResult>),
    Ready(PendingAuthResult),
}

#[cfg(all(feature = "browser", target_arch = "wasm32"))]
pub(crate) enum PendingAuthStatus {
    Pending,
    Ready(Result<TokenResponse, String>),
}

/// Internal runtime state for an in-flight auth operation.
#[derive(Resource)]
pub(crate) struct PendingAuthState {
    pub mode: PendingAuthMode,
    pub status: PendingAuthStatus,
}

/// Stores whether a connection request should resume after auth completes.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PendingAuthResume {
    /// Whether an auth-gated connection request is waiting to resume.
    pub requested: bool,
    /// The auth mode that should be used for the pending resolution.
    pub mode: PendingAuthMode,
}

#[cfg(all(feature = "browser", target_arch = "wasm32"))]
#[derive(Resource, Default)]
pub(crate) struct WasmPendingAuthResult(pub Option<Result<TokenResponse, String>>);

#[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
fn begin_pending_auth<F>(commands: &mut Commands, mode: PendingAuthMode, work: F)
where
    F: Send + 'static + FnOnce() -> Result<TokenResponse, String>,
{
    let (tx, rx) = bounded(1);

    println!(
        "bevy_stdb auth: starting pending auth task with mode: {:?}",
        mode
    );

    std::thread::spawn(move || {
        let result = work();
        let _ = tx.send(result);
    });

    commands.insert_resource(PendingAuthState {
        mode,
        status: PendingAuthStatus::Pending(rx),
    });
}

#[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
fn poll_pending_auth_result(pending_auth_state: &mut PendingAuthState) {
    let next_status = match &pending_auth_state.status {
        PendingAuthStatus::Ready(_) => None,
        PendingAuthStatus::Pending(rx) => match rx.try_recv() {
            Ok(result) => {
                println!(
                    "bevy_stdb auth: pending auth task completed for mode: {:?}",
                    pending_auth_state.mode
                );
                Some(PendingAuthStatus::Ready(result))
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                println!(
                    "bevy_stdb auth: pending auth task disconnected unexpectedly for mode: {:?}",
                    pending_auth_state.mode
                );
                Some(PendingAuthStatus::Ready(Err(
                    "The authentication flow ended unexpectedly before returning a result."
                        .to_string(),
                )))
            }
        },
    };

    if let Some(status) = next_status {
        pending_auth_state.status = status;
    }
}

/// Installs shared auth resources and systems.
pub(crate) struct StdbAuthPlugin<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    options: StdbAuthOptions,
    _marker: PhantomData<(C, M)>,
}

impl<C: DbConnection<Module = M> + DbContext + Send + Sync, M: SpacetimeModule<DbConnection = C>>
    StdbAuthPlugin<C, M>
{
    /// Creates a new [`StdbAuthPlugin`].
    pub(crate) fn new(options: StdbAuthOptions) -> Self {
        Self {
            options,
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
        app.add_message::<RequestLoginMessage>();
        app.add_message::<RequestLogoutMessage>();
        app.add_message::<AuthSuccessMessage>();
        app.add_message::<AuthFailureMessage>();

        app.insert_resource(StdbAuthConfig {
            options: self.options.clone(),
        });
        app.init_resource::<CurrentAuthTokens>();
        app.init_resource::<PendingAuthResume>();

        #[cfg(all(feature = "browser", target_arch = "wasm32"))]
        app.init_resource::<WasmPendingAuthResult>();

        #[cfg(all(feature = "browser", target_arch = "wasm32"))]
        app.add_systems(
            PreUpdate,
            resume_browser_callback_flow.in_set(StdbSet::StateSync),
        );

        app.add_systems(
            PreUpdate,
            (
                handle_login_requests,
                handle_logout_requests::<C>,
                start_pending_auth,
                poll_pending_auth,
                handle_auth_failure_messages,
                handle_auth_success_messages,
                sync_connected_token,
            )
                .chain()
                .in_set(StdbSet::StateSync),
        );
    }
}

/// Updates auth runtime from login requests.
fn handle_login_requests(
    mut login_requests: MessageReader<RequestLoginMessage>,
    pending_auth_state: Option<Res<PendingAuthState>>,
    mut pending_resume: ResMut<PendingAuthResume>,
    mut commands: Commands,
) {
    if login_requests.read().next().is_none() {
        return;
    }

    if pending_auth_state.is_some() {
        return;
    }

    pending_resume.requested = false;
    pending_resume.mode = PendingAuthMode::Interactive;

    commands.insert_resource(PendingAuthState {
        mode: PendingAuthMode::Interactive,
        #[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
        status: PendingAuthStatus::Ready(Err(
            "Interactive auth start must be implemented by the platform auth module.".to_string(),
        )),
        #[cfg(all(feature = "browser", target_arch = "wasm32"))]
        status: PendingAuthStatus::Ready(Err(
            "Interactive auth start must be implemented by the platform auth module.".to_string(),
        )),
    });
}

/// Updates auth runtime from logout requests.
fn handle_logout_requests<C: DbContext + Send + Sync + 'static>(
    mut logout_requests: MessageReader<RequestLogoutMessage>,
    auth_config: Res<StdbAuthConfig>,
    mut pending_resume: ResMut<PendingAuthResume>,
    mut tokens: ResMut<CurrentAuthTokens>,
    pending_auth_state: Option<Res<PendingAuthState>>,
    active_connection: Option<Res<StdbConnection<C>>>,
    mut commands: Commands,
    #[cfg(all(feature = "browser", target_arch = "wasm32"))] mut wasm_result: ResMut<
        WasmPendingAuthResult,
    >,
) {
    if logout_requests.read().next().is_none() {
        return;
    }

    println!("bevy_stdb auth: processing logout request");

    pending_resume.requested = false;
    pending_resume.mode = PendingAuthMode::Silent;
    tokens.clear();

    if pending_auth_state.is_some() {
        println!("bevy_stdb auth: clearing pending auth state during logout");
        commands.remove_resource::<PendingAuthState>();
    }

    #[cfg(all(feature = "browser", target_arch = "wasm32"))]
    {
        wasm_result.0 = None;
    }

    match auth_imp::clear_stored_tokens(&auth_config.options) {
        Ok(()) => println!("bevy_stdb auth: cleared stored auth tokens"),
        Err(error) => println!("bevy_stdb auth: failed to clear stored auth tokens: {error}"),
    }

    if let Some(connection) = active_connection {
        println!("bevy_stdb auth: disconnecting active connection during logout");
        let _ = connection.disconnect();
    }
}

#[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
fn start_pending_auth(
    auth_config: Res<StdbAuthConfig>,
    current_tokens: Res<CurrentAuthTokens>,
    pending_auth_state: Option<Res<PendingAuthState>>,
    mut commands: Commands,
) {
    let Some(pending_auth_state) = pending_auth_state else {
        return;
    };

    let PendingAuthStatus::Ready(result) = &pending_auth_state.status else {
        return;
    };

    if !matches!(
        result,
        Err(message) if message == "Interactive auth start must be implemented by the platform auth module."
    ) {
        return;
    }

    let has_in_memory_refresh_token = current_tokens.refresh_token().is_some();

    println!(
        "bevy_stdb auth: resolving auth with mode {:?}, in-memory refresh token present: {}",
        pending_auth_state.mode, has_in_memory_refresh_token
    );

    let options = auth_config.options.clone();
    let mode = pending_auth_state.mode;
    let current_tokens = current_tokens.clone();

    begin_pending_auth(&mut commands, mode, move || {
        auth_imp::resolve_auth(&options, mode, &current_tokens)
    });
}

#[cfg(all(feature = "browser", target_arch = "wasm32"))]
fn start_pending_auth(world: &mut bevy_ecs::world::World) {
    let mut system_state: SystemState<(
        Res<StdbAuthConfig>,
        Res<CurrentAuthTokens>,
        ResMut<WasmPendingAuthResult>,
        Option<Res<PendingAuthState>>,
        Commands,
    )> = SystemState::new(world);

    let (auth_config, current_tokens, mut wasm_result, pending_auth_state, mut commands) =
        system_state.get_mut(world);

    let Some(pending_auth_state) = pending_auth_state else {
        system_state.apply(world);
        return;
    };

    let PendingAuthStatus::Ready(result) = &pending_auth_state.status else {
        system_state.apply(world);
        return;
    };

    if !matches!(
        result,
        Err(message) if message == "Interactive auth start must be implemented by the platform auth module."
    ) {
        system_state.apply(world);
        return;
    }

    if let Some(result) = auth_imp::try_resolve_auth_now(
        &auth_config.options,
        pending_auth_state.mode,
        &current_tokens,
    ) {
        commands.insert_resource(PendingAuthState {
            mode: pending_auth_state.mode,
            status: PendingAuthStatus::Ready(result),
        });
    } else if matches!(pending_auth_state.mode, PendingAuthMode::Interactive) {
        wasm_result.0 = None;
        commands.insert_resource(PendingAuthState {
            mode: PendingAuthMode::Interactive,
            status: PendingAuthStatus::Pending,
        });
    }

    system_state.apply(world);
}

#[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
fn poll_pending_auth(
    mut pending_auth_state: Option<ResMut<PendingAuthState>>,
    auth_config: Res<StdbAuthConfig>,
    mut current_tokens: ResMut<CurrentAuthTokens>,
    mut success_messages: MessageWriter<AuthSuccessMessage>,
    mut failure_messages: MessageWriter<AuthFailureMessage>,
    mut commands: Commands,
) {
    let Some(mut pending_auth_state) = pending_auth_state.as_mut() else {
        return;
    };

    poll_pending_auth_result(&mut pending_auth_state);

    let PendingAuthStatus::Ready(result) = &pending_auth_state.status else {
        return;
    };

    let result = result.clone();
    let mode = pending_auth_state.mode;
    commands.remove_resource::<PendingAuthState>();

    match result {
        Ok(mut tokens) => {
            if tokens.refresh_token.is_none() {
                println!(
                    "bevy_stdb auth: auth result missing refresh token, preserving existing in-memory refresh token"
                );
                tokens.refresh_token = current_tokens.refresh_token().map(ToOwned::to_owned);
            }

            match auth_imp::store_tokens(&auth_config.options, &tokens) {
                Ok(()) => println!(
                    "bevy_stdb auth: stored tokens successfully, refresh token present: {}",
                    tokens.refresh_token.is_some()
                ),
                Err(error) => println!("bevy_stdb auth: failed to store tokens: {error}"),
            }

            println!(
                "bevy_stdb auth: auth succeeded for mode {:?}, refresh token present: {}",
                mode,
                tokens.refresh_token.is_some()
            );

            current_tokens.replace(tokens.clone());
            success_messages.write(AuthSuccessMessage(tokens));
        }
        Err(message) => {
            if matches!(mode, PendingAuthMode::Silent) {
                println!(
                    "bevy_stdb auth: silent auth failed, automatically falling back to interactive auth: {}",
                    message
                );
                commands.insert_resource(PendingAuthState {
                    mode: PendingAuthMode::Interactive,
                    status: PendingAuthStatus::Ready(Err(
                        "Interactive auth start must be implemented by the platform auth module."
                            .to_string(),
                    )),
                });
                return;
            }

            println!(
                "bevy_stdb auth: auth failed for mode {:?}: {}",
                mode, message
            );
            failure_messages.write(AuthFailureMessage { message });
        }
    }
}

#[cfg(all(feature = "browser", target_arch = "wasm32"))]
fn poll_pending_auth(
    pending_auth_state: Option<Res<PendingAuthState>>,
    auth_config: Res<StdbAuthConfig>,
    mut current_tokens: ResMut<CurrentAuthTokens>,
    mut wasm_result: ResMut<WasmPendingAuthResult>,
    mut success_messages: MessageWriter<AuthSuccessMessage>,
    mut failure_messages: MessageWriter<AuthFailureMessage>,
    mut commands: Commands,
) {
    let Some(pending_auth_state) = pending_auth_state else {
        return;
    };

    let result = match &pending_auth_state.status {
        PendingAuthStatus::Ready(result) => Some(result.clone()),
        PendingAuthStatus::Pending => match wasm_result.0.take() {
            Some(Err(message)) if message == "__bevy_stdb_pending__" => {
                wasm_result.0 = Some(Err(message));
                None
            }
            other => other,
        },
    };

    let Some(result) = result else {
        return;
    };

    let mode = pending_auth_state.mode;
    commands.remove_resource::<PendingAuthState>();

    match result {
        Ok(mut tokens) => {
            if tokens.refresh_token.is_none() {
                println!(
                    "bevy_stdb auth: auth result missing refresh token, preserving existing in-memory refresh token"
                );
                tokens.refresh_token = current_tokens.refresh_token().map(ToOwned::to_owned);
            }

            match auth_imp::store_tokens(&auth_config.options, &tokens) {
                Ok(()) => println!(
                    "bevy_stdb auth: stored tokens successfully, refresh token present: {}",
                    tokens.refresh_token.is_some()
                ),
                Err(error) => println!("bevy_stdb auth: failed to store tokens: {error}"),
            }

            println!(
                "bevy_stdb auth: auth succeeded for mode {:?}, refresh token present: {}",
                mode,
                tokens.refresh_token.is_some()
            );

            current_tokens.replace(tokens.clone());
            success_messages.write(AuthSuccessMessage(tokens));
        }
        Err(message) => {
            if matches!(mode, PendingAuthMode::Silent) {
                println!(
                    "bevy_stdb auth: silent auth failed, automatically falling back to interactive auth: {}",
                    message
                );
                commands.insert_resource(PendingAuthState {
                    mode: PendingAuthMode::Interactive,
                    status: PendingAuthStatus::Ready(Err(
                        "Interactive auth start must be implemented by the platform auth module."
                            .to_string(),
                    )),
                });
                return;
            }

            if matches!(mode, PendingAuthMode::Interactive)
                && message
                    == "Interactive auth start must be implemented by the platform auth module."
            {
                println!(
                    "bevy_stdb auth: browser interactive auth redirected before returning a token"
                );
                return;
            }

            println!(
                "bevy_stdb auth: auth failed for mode {:?}: {}",
                mode, message
            );
            failure_messages.write(AuthFailureMessage { message });
        }
    }
}

#[cfg(all(feature = "browser", target_arch = "wasm32"))]
fn resume_browser_callback_flow(world: &mut bevy_ecs::world::World) {
    if world.contains_resource::<PendingAuthState>() {
        return;
    }

    let status = match auth_imp::callback_status() {
        Ok(status) => status,
        Err(error) => {
            world.insert_resource(PendingAuthState {
                mode: PendingAuthMode::Interactive,
                status: PendingAuthStatus::Ready(Err(error.to_string())),
            });
            return;
        }
    };

    match status {
        auth_imp::WebAuthCallbackStatus::None => {}
        auth_imp::WebAuthCallbackStatus::Failure { message } => {
            let _ = auth_imp::clear_callback_query();
            world.insert_resource(PendingAuthState {
                mode: PendingAuthMode::Interactive,
                status: PendingAuthStatus::Ready(Err(message)),
            });
        }
        auth_imp::WebAuthCallbackStatus::Ready => {
            let options = world
                .get_resource::<StdbAuthConfig>()
                .expect("StdbAuthConfig should exist before resuming browser auth callback")
                .options
                .clone();

            if let Some(mut result_slot) = world.get_resource_mut::<WasmPendingAuthResult>() {
                result_slot.0 = None;
            }

            match auth_imp::begin_callback_exchange(options) {
                Ok(()) => {
                    world.insert_resource(PendingAuthState {
                        mode: PendingAuthMode::Interactive,
                        status: PendingAuthStatus::Pending,
                    });
                }
                Err(message) => {
                    world.insert_resource(PendingAuthState {
                        mode: PendingAuthMode::Interactive,
                        status: PendingAuthStatus::Ready(Err(message)),
                    });
                }
            }
        }
    }
}

/// Updates auth runtime from auth failures.
fn handle_auth_failure_messages(
    mut failures: MessageReader<AuthFailureMessage>,
    mut pending_resume: ResMut<PendingAuthResume>,
) {
    if failures.read().next().is_none() {
        return;
    }

    pending_resume.requested = false;
}

/// Updates auth runtime from successful auth results.
fn handle_auth_success_messages(
    mut successes: MessageReader<AuthSuccessMessage>,
    mut pending_resume: ResMut<PendingAuthResume>,
    mut tokens: ResMut<CurrentAuthTokens>,
    mut controller: Option<ResMut<StdbConnectionController>>,
) {
    let Some(response) = successes.read().last().map(|msg| msg.0.clone()) else {
        return;
    };

    let access_token = response.access_token.clone();

    println!(
        "bevy_stdb auth: received auth success message, pending connect: {}, refresh token present: {}",
        pending_resume.requested,
        response.refresh_token.is_some()
    );

    tokens.replace(response);

    if pending_resume.requested {
        pending_resume.requested = false;

        if let Some(controller) = controller.as_mut() {
            println!("bevy_stdb auth: resuming pending connection with resolved access token");
            controller.connect_with_token(access_token);
        }
    }
}

/// Updates the stored access token from successful connections.
fn sync_connected_token(
    mut connected: MessageReader<StdbConnectedMessage>,
    mut pending_resume: ResMut<PendingAuthResume>,
    mut tokens: ResMut<CurrentAuthTokens>,
) {
    let Some(access_token) = connected.read().last().map(|msg| msg.access_token.clone()) else {
        return;
    };

    println!("bevy_stdb auth: syncing access token from successful SpacetimeDB connection");
    pending_resume.requested = false;
    tokens.set_access_token(access_token);
}

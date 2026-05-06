#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
use crate::auth::{
    StdbAuthSource,
    plugin::{
        LoginOutcome, PendingLogin, PendingTokenRefresh, StdbAuthRefresh,
        acquire_login_token_response, clear_stored_refresh_token,
    },
};
use crate::connection::{
    PendingConnection, PendingConnectionPhase, StdbConnection, StdbConnectionConfig,
};
use bevy_ecs::{
    prelude::{Commands, Res, ResMut},
    system::SystemParam,
};
use bevy_tasks::IoTaskPool;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
/// Options for authenticating with SpacetimeDB.
#[derive(Clone, Debug)]
pub struct StdbLoginOptions {
    /// The authentication source used to acquire an access token.
    pub auth_source: StdbAuthSource,
}

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
impl StdbLoginOptions {
    /// Creates [`StdbLoginOptions`] with the given [`StdbAuthSource`].
    pub fn new(auth_source: StdbAuthSource) -> Self {
        Self { auth_source }
    }
}

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
/// Options for clearing stored SpacetimeDB authentication.
#[derive(Clone, Debug)]
pub struct StdbLogoutOptions {
    /// Also clears the stored refresh token when `true`.
    pub clear_stored_refresh_token: bool,
}

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
impl Default for StdbLogoutOptions {
    fn default() -> Self {
        Self {
            clear_stored_refresh_token: true,
        }
    }
}

/// Options for starting a SpacetimeDB connection attempt.
#[derive(Clone, Debug, Default)]
pub struct StdbConnectOptions {
    /// Optional access token for this connection attempt.
    pub token: Option<String>,
    /// Optional URI for this connection attempt.
    pub uri: Option<String>,
    /// Optional module name for this connection attempt.
    pub module_name: Option<String>,
}

impl StdbConnectOptions {
    /// Creates [`StdbConnectOptions`] with an access token.
    pub fn from_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            uri: None,
            module_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI.
    pub fn from_uri(uri: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            module_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a module name.
    pub fn from_module_name(module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: None,
            module_name: Some(module_name.into()),
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI and module name.
    pub fn from_target(uri: impl Into<String>, module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            module_name: Some(module_name.into()),
        }
    }
}

/// Sends SpacetimeDB commands from Bevy systems.
#[derive(SystemParam)]
pub struct StdbCommands<'w, 's, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    config: ResMut<'w, StdbConnectionConfig<C, M>>,
    connection: Option<Res<'w, StdbConnection<C>>>,
    pending: Option<Res<'w, PendingConnection<C>>>,
    commands: Commands<'w, 's>,
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pending_login: Option<Res<'w, PendingLogin>>,
}

impl<C, M> StdbCommands<'_, '_, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    /// Requests authentication using [`StdbLoginOptions`].
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub fn login(&mut self, options: StdbLoginOptions) {
        if self.pending_login.is_some() {
            return;
        }
        let auth_source = options.auth_source;
        let client_id = auth_source.client_id();
        let task = IoTaskPool::get().spawn(async move {
            let token_response = acquire_login_token_response(&auth_source).await?;
            Ok(LoginOutcome {
                token_response,
                client_id,
            })
        });
        self.commands.insert_resource(PendingLogin(task));
    }

    /// Requests stored authentication to be cleared using [`StdbLogoutOptions`].
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub fn logout(&mut self, options: StdbLogoutOptions) {
        if options.clear_stored_refresh_token {
            if let Some(client_id) = self.config.client_id() {
                clear_stored_refresh_token(client_id);
            }
        }
        self.config.clear_auth();
        self.commands.remove_resource::<StdbAuthRefresh>();
        self.commands.remove_resource::<PendingLogin>();
        self.commands.remove_resource::<PendingTokenRefresh>();
    }

    /// Spawns a connection task immediately using [`StdbConnectOptions`].
    ///
    /// No-op if a connection attempt is already pending.
    pub fn connect(&mut self, options: StdbConnectOptions) {
        if self.pending.is_some() {
            return;
        }

        if let Some(uri) = options.uri {
            self.config.uri = uri;
        }
        if let Some(module_name) = options.module_name {
            self.config.module_name = module_name;
        }
        if let Some(token) = options.token {
            self.config.token = Some(token);
        }

        let config = self.config.clone();
        let task = IoTaskPool::get().spawn(async move { config.build_connection().await });
        self.commands
            .insert_resource(PendingConnection::<C>::new(PendingConnectionPhase::Build(
                task,
            )));
    }

    /// Disconnects from the active SpacetimeDB.
    pub fn disconnect(&mut self) {
        if let Some(conn) = &self.connection {
            let _ = conn.disconnect();
        }
        self.commands.remove_resource::<StdbConnection<C>>();
        self.commands.remove_resource::<PendingConnection<C>>();
    }
}

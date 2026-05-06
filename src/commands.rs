use crate::connection::{
    PendingConnection, PendingConnectionPhase, StdbConnection, StdbConnectionConfig,
};
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
use crate::{
    auth::StdbAuthSource,
    message::{StdbLoginRequest, StdbLogoutRequest},
};
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
use bevy_ecs::prelude::MessageWriter;
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
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            uri: None,
            module_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI.
    pub fn with_uri(uri: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            module_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a module name.
    pub fn with_module_name(module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: None,
            module_name: Some(module_name.into()),
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI and module name.
    pub fn with_target(uri: impl Into<String>, module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            module_name: Some(module_name.into()),
        }
    }
}

/// Options for disconnecting from SpacetimeDB.
#[derive(Clone, Debug, Default)]
pub struct StdbDisconnectOptions;

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
    login_requests: MessageWriter<'w, StdbLoginRequest>,
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    logout_requests: MessageWriter<'w, StdbLogoutRequest>,
}

impl<C, M> StdbCommands<'_, '_, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    /// Requests authentication using [`StdbLoginOptions`].
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub fn login(&mut self, options: StdbLoginOptions) {
        self.login_requests.write(StdbLoginRequest {
            auth_source: options.auth_source,
        });
    }

    /// Requests stored authentication to be cleared using [`StdbLogoutOptions`].
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub fn logout(&mut self, options: StdbLogoutOptions) {
        self.logout_requests.write(StdbLogoutRequest {
            clear_stored_refresh_token: options.clear_stored_refresh_token,
        });
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

    /// Disconnects from SpacetimeDB using [`StdbDisconnectOptions`].
    pub fn disconnect(&mut self, _options: StdbDisconnectOptions) {
        if let Some(conn) = &self.connection {
            let _ = conn.disconnect();
        }
        self.commands.remove_resource::<StdbConnection<C>>();
        self.commands.remove_resource::<PendingConnection<C>>();
    }
}

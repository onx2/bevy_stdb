//! Consolidated command interface for SpacetimeDB connection and authentication.

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
use crate::auth::{
    StdbAuthSource,
    plugin::{LoginOutcome, PendingAuth, acquire_login_token_response, end_session},
};
use crate::connection::{PendingConnection, StdbConnection, StdbConnectionConfig};
use bevy_ecs::{
    prelude::{Commands, Res, ResMut},
    system::SystemParam,
};
use bevy_tasks::IoTaskPool;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};

/// Options for starting a SpacetimeDB connection attempt.
#[derive(Clone, Debug, Default)]
pub struct StdbConnectOptions {
    /// Optional access token for this connection attempt.
    pub token: Option<String>,
    /// Optional URI for this connection attempt.
    pub uri: Option<String>,
    /// Optional module name for this connection attempt.
    pub database_name: Option<String>,
}

impl StdbConnectOptions {
    /// Creates [`StdbConnectOptions`] with an access token.
    pub fn from_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            uri: None,
            database_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI.
    pub fn from_uri(uri: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            database_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a module name.
    pub fn from_module_name(module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: None,
            database_name: Some(module_name.into()),
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI and module name.
    pub fn from_target(uri: impl Into<String>, module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            database_name: Some(module_name.into()),
        }
    }
}

/// Options for authenticating with SpacetimeDB.
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
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

/// Options for clearing stored SpacetimeDB authentication.
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
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

/// Sends SpacetimeDB connection and authentication commands from Bevy systems.
#[derive(SystemParam)]
pub struct StdbCommands<'w, 's, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    config: ResMut<'w, StdbConnectionConfig<C, M>>,
    connection: Option<Res<'w, StdbConnection<C>>>,
    pending_connection: Option<Res<'w, PendingConnection<C>>>,
    commands: Commands<'w, 's>,
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pending_auth: Option<Res<'w, PendingAuth>>,
}

impl<C, M> StdbCommands<'_, '_, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    /// Spawns a connection task using [`StdbConnectOptions`].
    ///
    /// No-op if a [`StdbConnection`] exists or a connection attempt is already in flight.
    pub fn connect(&mut self, options: StdbConnectOptions) {
        if self.connection.is_some() || self.pending_connection.is_some() {
            return;
        }
        self.connect_impl(options);
    }

    /// Disconnects any active or pending connection, then spawns a new connection task.
    pub fn reconnect(&mut self, options: StdbConnectOptions) {
        self.disconnect();
        self.connect_impl(options);
    }

    fn connect_impl(&mut self, options: StdbConnectOptions) {
        if let Some(uri) = options.uri {
            self.config.uri = uri;
        }
        if let Some(database_name) = options.database_name {
            self.config.database_name = database_name;
        }
        if let Some(token) = options.token {
            self.config.token = Some(token);
        }

        let config = self.config.clone();
        let task = IoTaskPool::get().spawn(async move { config.build_connection().await });
        self.commands.insert_resource(PendingConnection::<C>(task));
    }

    /// Disconnects from the active SpacetimeDB connection.
    pub fn disconnect(&mut self) {
        if let Some(conn) = &self.connection {
            let _ = conn.disconnect();
        }
        self.commands.remove_resource::<StdbConnection<C>>();
        self.commands.remove_resource::<PendingConnection<C>>();
    }
}

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
impl<C, M> StdbCommands<'_, '_, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    /// Spawns a login task immediately using [`StdbLoginOptions`].
    ///
    /// A successful login updates the connection configuration with the new access token.
    ///
    /// No-op if a login attempt is already pending.
    pub fn login(&mut self, options: StdbLoginOptions) {
        if self.pending_auth.is_some() {
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
        self.commands.insert_resource(PendingAuth::Login(task));
    }

    /// Initiates an async logout, ending the Spacetime Auth session and clearing local auth state.
    ///
    /// No-op if a logout attempt is already pending.
    pub fn logout(&mut self, options: StdbLogoutOptions) {
        if self.pending_auth.is_some() {
            return;
        }

        let Some(client_id) = self.config.client_id.clone() else {
            return;
        };
        let id_token = self.config.id_token.clone();
        let clear_refresh_token = options.clear_stored_refresh_token;

        let task = IoTaskPool::get()
            .spawn(async move { end_session(&client_id, id_token.as_deref()).await });

        self.commands.insert_resource(PendingAuth::Logout {
            task,
            clear_refresh_token,
        });
    }
}

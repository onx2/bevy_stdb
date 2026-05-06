use super::{PendingConnection, PendingConnectionPhase, StdbConnection, StdbConnectionConfig};
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

/// Sends SpacetimeDB connection commands from Bevy systems.
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
}

impl<C, M> StdbCommands<'_, '_, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
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

    /// Disconnects from the active SpacetimeDB connection.
    pub fn disconnect(&mut self) {
        if let Some(conn) = &self.connection {
            let _ = conn.disconnect();
        }
        self.commands.remove_resource::<StdbConnection<C>>();
        self.commands.remove_resource::<PendingConnection<C>>();
    }
}

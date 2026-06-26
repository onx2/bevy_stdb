use crate::connection::{PendingConnection, StdbConnection, StdbConnectionConfig};
use bevy_ecs::{
    prelude::{Command, Commands, World},
    system::SystemParam,
};
use bevy_tasks::IoTaskPool;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::marker::PhantomData;

/// Options for starting a SpacetimeDB connection attempt.
#[derive(Clone, Debug, Default)]
pub struct StdbConnectOptions {
    /// Optional access token for this connection attempt.
    pub token: Option<String>,
    /// Optional URI for this connection attempt.
    pub uri: Option<String>,
    /// Optional database name for this connection attempt.
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

    /// Creates [`StdbConnectOptions`] with a database name.
    pub fn from_database_name(database_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: None,
            database_name: Some(database_name.into()),
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI and database name.
    pub fn from_target(uri: impl Into<String>, database_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            database_name: Some(database_name.into()),
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
    commands: Commands<'w, 's>,
    _marker: PhantomData<fn() -> (C, M)>,
}

impl<C, M> StdbCommands<'_, '_, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    /// Requests a SpacetimeDB connection attempt using [`StdbConnectOptions`].
    ///
    /// No-op if a [`StdbConnection`] exists or a connection attempt is already in flight.
    pub fn connect(&mut self, options: StdbConnectOptions) {
        self.commands
            .queue(StartConnectCommand::<C, M>::new(options));
    }

    /// Requests a new connection after closing the active or pending connection.
    pub fn reconnect(&mut self, options: StdbConnectOptions) {
        self.commands.queue(ReconnectCommand::<C, M>::new(options));
    }

    /// Requests disconnection from the active SpacetimeDB connection.
    pub fn disconnect(&mut self) {
        self.commands.queue(DisconnectCommand::<C>::new());
    }

    /// Sets the authentication token in the SpacetimeDB connection config.
    ///
    /// This does not immediately reconnect with the new token but instead
    /// will be used for future connection attempts.
    pub fn set_token(&mut self, token: impl Into<String>) {
        let token = token.into();
        self.commands.queue(move |world: &mut World| {
            if let Some(mut config) = world.get_resource_mut::<StdbConnectionConfig<C, M>>() {
                config.token = Some(token);
            }
        });
    }
}

/// A command that starts a SpacetimeDB connection attempt.
pub(crate) struct StartConnectCommand<C, M> {
    options: StdbConnectOptions,
    _marker: PhantomData<fn() -> (C, M)>,
}

impl<C, M> StartConnectCommand<C, M> {
    /// Creates [`StartConnectCommand`] with [`StdbConnectOptions`].
    pub(crate) fn new(options: StdbConnectOptions) -> Self {
        Self {
            options,
            _marker: PhantomData,
        }
    }
}

impl<C, M> Command for StartConnectCommand<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    type Out = ();
    fn apply(self, world: &mut World) {
        if world.contains_resource::<StdbConnection<C>>()
            || world.contains_resource::<PendingConnection<C>>()
        {
            return;
        }

        spawn_connection_task::<C, M>(world, self.options);
    }
}

struct ReconnectCommand<C, M> {
    options: StdbConnectOptions,
    _marker: PhantomData<fn() -> (C, M)>,
}

impl<C, M> ReconnectCommand<C, M> {
    fn new(options: StdbConnectOptions) -> Self {
        Self {
            options,
            _marker: PhantomData,
        }
    }
}

impl<C, M> Command for ReconnectCommand<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    type Out = ();
    fn apply(self, world: &mut World) {
        disconnect_connection::<C>(world);
        spawn_connection_task::<C, M>(world, self.options);
    }
}

struct DisconnectCommand<C> {
    _marker: PhantomData<fn() -> C>,
}

impl<C> DisconnectCommand<C> {
    fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<C> Command for DisconnectCommand<C>
where
    C: DbContext + Send + Sync + 'static,
{
    type Out = ();
    fn apply(self, world: &mut World) {
        disconnect_connection::<C>(world);
    }
}

fn spawn_connection_task<C, M>(world: &mut World, options: StdbConnectOptions)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    let config = {
        let mut config = world.resource_mut::<StdbConnectionConfig<C, M>>();

        if let Some(uri) = options.uri {
            config.uri = uri;
        }
        if let Some(database_name) = options.database_name {
            config.database_name = database_name;
        }
        if let Some(token) = options.token {
            config.token = Some(token);
        }

        config.clone()
    };

    let task = IoTaskPool::get().spawn(async move { config.build_connection().await });
    world.insert_resource(PendingConnection::<C>(task));
}

fn disconnect_connection<C>(world: &mut World)
where
    C: DbContext + Send + Sync + 'static,
{
    if let Some(conn) = world.get_resource::<StdbConnection<C>>() {
        let _ = conn.disconnect();
    }

    world.remove_resource::<StdbConnection<C>>();
    world.remove_resource::<PendingConnection<C>>();
}

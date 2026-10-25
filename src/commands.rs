use crate::connection::{
    PendingConnection, StdbConnection, StdbConnectionConfig, cancel_retry, close_connection,
    end_reconnect_cycle, start_attempt,
};
use bevy_ecs::{
    prelude::{Command, Commands, World},
    system::SystemParam,
};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::marker::PhantomData;

/// Connection settings to override when a connection attempt starts.
///
/// Each field that is `Some` replaces the stored setting for this attempt and every later one,
/// including automatic reconnects. Fields left `None` keep the current setting.
#[derive(Clone, Debug, Default)]
pub struct StdbConnectOptions {
    /// The access token to connect with.
    pub token: Option<String>,
    /// The URI to connect to.
    pub uri: Option<String>,
    /// The database name to connect to.
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
    /// This ends any reconnect cycle in progress, including a retry attempt in flight, so the new
    /// attempt uses `options` and a failure is retried with the full
    /// [`StdbReconnectOptions`](crate::prelude::StdbReconnectOptions) budget. Does nothing more if
    /// a [`StdbConnection`] exists or another attempt is already in flight.
    pub fn connect(&mut self, options: StdbConnectOptions) {
        self.commands.queue(move |world: &mut World| {
            end_reconnect_cycle(world);
            cancel_retry::<C>(world);
            StartConnectCommand::<C, M>::new(options).apply(world);
        });
    }

    /// Requests a new connection after closing the active or pending connection.
    ///
    /// The close is reported as [`DisconnectIntent::Requested`](crate::prelude::DisconnectIntent),
    /// and stored subscriptions are applied to the new connection.
    pub fn reconnect(&mut self, options: StdbConnectOptions) {
        self.commands.queue(ReconnectCommand::<C, M>::new(options));
    }

    /// Requests disconnection from the active SpacetimeDB connection.
    ///
    /// The close is reported as [`DisconnectIntent::Requested`](crate::prelude::DisconnectIntent)
    /// and is not retried. This also cancels any pending reconnect attempt.
    pub fn disconnect(&mut self) {
        self.commands.queue(DisconnectCommand::<C, M>::new());
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
    /// Whether the reconnect timer asked for this attempt.
    retry: bool,
    _marker: PhantomData<fn() -> (C, M)>,
}

impl<C, M> StartConnectCommand<C, M> {
    /// Creates [`StartConnectCommand`] with [`StdbConnectOptions`].
    pub(crate) fn new(options: StdbConnectOptions) -> Self {
        Self {
            options,
            retry: false,
            _marker: PhantomData,
        }
    }

    /// Creates [`StartConnectCommand`] for an attempt the reconnect timer asked for.
    pub(crate) fn retry() -> Self {
        Self {
            options: StdbConnectOptions::default(),
            retry: true,
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

        start_attempt::<C, M>(world, self.options, self.retry);
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
        close_connection::<C, M>(world);
        start_attempt::<C, M>(world, self.options, false);
    }
}

struct DisconnectCommand<C, M> {
    _marker: PhantomData<fn() -> (C, M)>,
}

impl<C, M> DisconnectCommand<C, M> {
    fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<C, M> Command for DisconnectCommand<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    type Out = ();
    fn apply(self, world: &mut World) {
        close_connection::<C, M>(world);
    }
}

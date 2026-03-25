//! Connection state and resources.
//!
//! This module manages the active connection and its Bevy lifecycle integration.

use crate::{
    alias::{
        ReadStdbConnectedMessage, ReadStdbConnectionErrorMessage, ReadStdbDisconnectedMessage,
    },
    channel_bridge::register_channel,
    message::{StdbConnectedMessage, StdbConnectionErrorMessage, StdbDisconnectedMessage},
    table::{TableRegistrar, TableRegistrarCallback},
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::{resource::Resource, system::ResMut};

use bevy_state::{
    app::{AppExtStates, StatesPlugin},
    state::{NextState, OnEnter, States},
};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, ConnectionId, DbConnectionBuilder, DbContext, Identity, Result,
};
use std::{
    sync::{Arc, mpsc::Sender},
    thread::JoinHandle,
};

/// Lifecycle state for the active SpacetimeDB connection.
#[derive(States, Debug, Default, Clone, PartialEq, Eq, Hash)]
pub enum StdbConnectionState {
    /// The plugin hasn't initialized yet.
    #[default]
    Uninitialized,

    /// The connection is active.
    Connected,

    /// The connection is not active.
    Disconnected,

    /// A reconnect attempt is in progress.
    Reconnecting,

    /// Reconnect attempts have been exhausted.
    Exhausted,
}

/// Runtime configuration for the active SpacetimeDB connection.
#[derive(Resource, Clone)]
pub(crate) struct StdbConnectionConfig<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    /// The remote module/database name.
    pub module_name: String,
    /// The URI of the SpacetimeDB host.
    pub uri: String,
    /// Optional authentication token.
    pub token: Option<String>,
    /// The function used to drive the connection.
    pub run_fn: fn(&C) -> JoinHandle<()>,
    /// Compression configuration for the connection.
    pub compression: Compression,
    /// Stored table registration closure for init and bind.
    pub table_registrar: Option<Arc<TableRegistrarCallback<C>>>,
    /// Sender used by the SpacetimeDB on-connect callback.
    pub connected_tx: Sender<StdbConnectedMessage>,
    /// Sender used by the SpacetimeDB on-disconnect callback.
    pub disconnected_tx: Sender<StdbDisconnectedMessage>,
    /// Sender used by the SpacetimeDB on-connect-error callback.
    pub error_tx: Sender<StdbConnectionErrorMessage>,
}

impl<C, M> StdbConnectionConfig<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    /// Builds a SpacetimeDB connection from this config.
    ///
    /// The returned connection is not started automatically.
    pub(crate) fn build_connection(&self) -> Result<Arc<C>> {
        let connected_tx = self.connected_tx.clone();
        let disconnected_tx = self.disconnected_tx.clone();
        let error_tx = self.error_tx.clone();

        DbConnectionBuilder::<M>::new()
            .with_database_name(self.module_name.clone())
            .with_uri(self.uri.clone())
            .with_token(self.token.clone())
            .with_compression(self.compression)
            .on_connect(move |_ctx, id, token| {
                let _ = connected_tx.send(StdbConnectedMessage {
                    identity: id,
                    access_token: token.to_string(),
                });
            })
            .on_disconnect(move |_ctx, err| {
                let _ = disconnected_tx.send(StdbDisconnectedMessage { err });
            })
            .on_connect_error(move |_ctx, err| {
                let _ = error_tx.send(StdbConnectionErrorMessage { err });
            })
            .build()
            .map(Arc::new)
    }
}

/// A Bevy resource for the active SpacetimeDB connection.
#[derive(Resource)]
pub struct StdbConnection<T: DbContext + 'static> {
    /// The underlying connection context.
    conn: Arc<T>,
}

impl<T: DbContext> StdbConnection<T> {
    /// Wraps an existing shared connection.
    pub(crate) fn new(conn: Arc<T>) -> Self {
        Self { conn }
    }
}

impl<T: DbContext> StdbConnection<T> {
    /// Returns the current database view.
    pub fn db(&self) -> &T::DbView {
        self.conn.db()
    }

    /// Returns access to the module reducers.
    pub fn reducers(&self) -> &T::Reducers {
        self.conn.reducers()
    }

    /// Returns access to the module procedures.
    pub fn procedures(&self) -> &T::Procedures {
        self.conn.procedures()
    }

    /// Returns `true` if the connection is currently active.
    pub fn is_active(&self) -> bool {
        self.conn.is_active()
    }

    /// Closes the connection to the SpacetimeDB server.
    pub fn disconnect(&self) -> Result<()> {
        self.conn.disconnect()
    }

    /// Returns a builder for database subscriptions.
    pub fn subscription_builder(&self) -> T::SubscriptionBuilder {
        self.conn.subscription_builder()
    }

    /// Returns the [`Identity`] of the current connection.
    pub fn identity(&self) -> Identity {
        self.conn.identity()
    }

    /// Returns the [`Identity`] of the current connection, if available.
    pub fn try_identity(&self) -> Option<Identity> {
        self.conn.try_identity()
    }

    /// Returns the current session's [`ConnectionId`].
    pub fn connection_id(&self) -> ConnectionId {
        self.conn.connection_id()
    }

    /// Returns the current session's [`ConnectionId`], if available.
    pub fn try_connection_id(&self) -> Option<ConnectionId> {
        self.conn.try_connection_id()
    }
}

/// Internal plugin for the SpacetimeDB connection lifecycle.
pub(crate) struct StdbConnectionPlugin<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    /// The remote module/database name.
    pub module_name: String,
    /// The URI of the SpacetimeDB host.
    pub uri: String,
    /// Optional authentication token.
    pub token: Option<String>,
    /// The function used to drive the connection.
    pub run_fn: fn(&C) -> JoinHandle<()>,
    /// Compression configuration for the connection.
    pub compression: Compression,
    /// Stored table registration closure for init and bind.
    pub table_registrar: Option<Arc<TableRegistrarCallback<C>>>,
}

impl<C: DbConnection<Module = M> + DbContext + Send + Sync, M: SpacetimeModule<DbConnection = C>>
    Plugin for StdbConnectionPlugin<C, M>
{
    /// Initializes connection state, resources, and lifecycle systems.
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<StatesPlugin>() {
            app.add_plugins(StatesPlugin);
        }
        app.init_state::<StdbConnectionState>();

        let config = StdbConnectionConfig::<C, M> {
            module_name: self.module_name.clone(),
            uri: self.uri.clone(),
            token: self.token.clone(),
            run_fn: self.run_fn,
            compression: self.compression,
            table_registrar: self.table_registrar.clone(),
            connected_tx: register_channel::<StdbConnectedMessage>(app),
            disconnected_tx: register_channel::<StdbDisconnectedMessage>(app),
            error_tx: register_channel::<StdbConnectionErrorMessage>(app),
        };
        app.insert_resource(config);

        app.add_systems(
            PreUpdate,
            (watch_connected, watch_disconnected, watch_connection_error),
        );
        app.add_systems(
            OnEnter(StdbConnectionState::Connected),
            on_connected_bind::<C, M>,
        );
    }

    /// Establishes the initial connection and registers table handlers.
    fn finish(&self, app: &mut App) {
        let (conn, table_registrar, run_fn) = {
            let config = app
                .world()
                .get_resource::<StdbConnectionConfig<C, M>>()
                .expect("StdbConnectionConfig should be inserted during plugin build");

            let conn = config
                .build_connection()
                .expect("Failed to establish initial connection");

            (conn, config.table_registrar.clone(), config.run_fn)
        };

        if let Some(register) = &table_registrar {
            let db = conn.db();
            register(&mut TableRegistrar::new_init(app), db);
        }

        run_fn(conn.as_ref());
        app.insert_resource(StdbConnection::new(conn));
    }
}

fn watch_connected(
    mut msgs: ReadStdbConnectedMessage,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
) {
    for _ in msgs.read() {
        next_state.set(StdbConnectionState::Connected);
    }
}

fn watch_disconnected(
    mut msgs: ReadStdbDisconnectedMessage,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
) {
    for _ in msgs.read() {
        next_state.set(StdbConnectionState::Disconnected);
    }
}

fn watch_connection_error(
    mut msgs: ReadStdbConnectionErrorMessage,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
) {
    for _ in msgs.read() {
        next_state.set(StdbConnectionState::Disconnected);
    }
}

fn on_connected_bind<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
>(
    world: &mut bevy_ecs::world::World,
) {
    let config = world
        .get_resource::<StdbConnectionConfig<C, M>>()
        .expect("StdbConnectionConfig should exist before Connected bind phase");
    let conn = world
        .get_resource::<StdbConnection<C>>()
        .expect("StdbConnection should exist before Connected bind phase");

    let db = conn.db();
    if let Some(register) = &config.table_registrar {
        register(&mut TableRegistrar::new_bind(&*world), db);
    }
}

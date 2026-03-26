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
use bevy_ecs::{
    resource::Resource,
    schedule::IntoScheduleConfigs,
    system::{Res, ResMut},
};
use bevy_state::{
    app::{AppExtStates, StatesPlugin},
    condition::in_state,
    state::{NextState, OnEnter, States},
};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, ConnectionId, DbConnectionBuilder, DbContext, Identity, Result,
};
use std::sync::{Arc, Mutex, mpsc::Sender};

#[cfg(feature = "browser")]
use std::sync::mpsc::{Receiver, channel};

#[derive(Resource)]
struct InitialConnectionState<C: DbContext + Send + Sync + 'static> {
    result: Mutex<Option<Result<Arc<C>>>>,
    #[cfg(feature = "browser")]
    rx: Mutex<Receiver<Result<Arc<C>>>>,
}

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
#[derive(Resource)]
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
    /// The function used to drive the connection from the Bevy schedule.
    pub frame_tick: Option<fn(&C) -> Result<()>>,
    /// The function used to start background connection processing.
    pub background_driver: Option<Arc<dyn Fn(&C) + Send + Sync>>,
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

impl<C, M> Clone for StdbConnectionConfig<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    fn clone(&self) -> Self {
        Self {
            module_name: self.module_name.clone(),
            uri: self.uri.clone(),
            token: self.token.clone(),
            frame_tick: self.frame_tick,
            background_driver: self.background_driver.clone(),
            compression: self.compression,
            table_registrar: self.table_registrar.clone(),
            connected_tx: self.connected_tx.clone(),
            disconnected_tx: self.disconnected_tx.clone(),
            error_tx: self.error_tx.clone(),
        }
    }
}

impl<C, M> StdbConnectionConfig<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    fn connection_builder(&self) -> DbConnectionBuilder<M> {
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
    }

    /// Builds a SpacetimeDB connection from this config.
    ///
    /// The returned connection is not started automatically.
    #[cfg(not(feature = "browser"))]
    pub(crate) fn build_connection(&self) -> Result<Arc<C>> {
        self.connection_builder().build().map(Arc::new)
    }

    /// Builds a SpacetimeDB connection from this config.
    ///
    /// The returned connection is not started automatically.
    #[cfg(feature = "browser")]
    pub(crate) async fn build_connection(&self) -> Result<Arc<C>> {
        self.connection_builder().build().await.map(Arc::new)
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
    /// The function used to drive the connection from the Bevy schedule.
    pub frame_tick: Option<fn(&C) -> Result<()>>,
    /// The function used to start background connection processing.
    pub background_driver: Option<Arc<dyn Fn(&C) + Send + Sync>>,
    /// Compression configuration for the connection.
    pub compression: Compression,
    /// Stored table registration closure for init and bind.
    pub table_registrar: Option<Arc<TableRegistrarCallback<C>>>,
}

impl<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
> Plugin for StdbConnectionPlugin<C, M>
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
            frame_tick: self.frame_tick,
            background_driver: self.background_driver.clone(),
            compression: self.compression,
            table_registrar: self.table_registrar.clone(),
            connected_tx: register_channel::<StdbConnectedMessage>(app),
            disconnected_tx: register_channel::<StdbDisconnectedMessage>(app),
            error_tx: register_channel::<StdbConnectionErrorMessage>(app),
        };

        let initial_connection_state = {
            #[cfg(feature = "browser")]
            {
                let pending_config = config.clone();
                let (tx, rx) = channel();

                tracing::info!("bevy_stdb: spawning initial browser connection task");
                wasm_bindgen_futures::spawn_local(async move {
                    tracing::info!("bevy_stdb: initial browser connection task started");
                    let result = pending_config.build_connection().await;
                    tracing::info!(
                        "bevy_stdb: initial browser connection task completed: success={}",
                        result.is_ok()
                    );
                    let _ = tx.send(result);
                });

                InitialConnectionState::<C> {
                    result: Mutex::new(None),
                    rx: Mutex::new(rx),
                }
            }

            #[cfg(not(feature = "browser"))]
            {
                InitialConnectionState::<C> {
                    result: Mutex::new(Some(config.build_connection())),
                }
            }
        };

        app.insert_resource(config);
        app.insert_resource(initial_connection_state);

        app.add_systems(
            PreUpdate,
            (
                watch_connected::<C, M>,
                watch_disconnected,
                watch_connection_error,
                drive_connection_frame_tick::<C, M>
                    .run_if(in_state(StdbConnectionState::Connected)),
            ),
        );
        app.add_systems(
            OnEnter(StdbConnectionState::Connected),
            on_connected_bind::<C, M>,
        );
    }

    #[cfg(feature = "browser")]
    fn ready(&self, app: &App) -> bool {
        let state = app
            .world()
            .get_resource::<InitialConnectionState<C>>()
            .expect("InitialConnectionState should be inserted during plugin build");

        {
            let result = state.result.lock().unwrap_or_else(|e| e.into_inner());
            if result.is_some() {
                tracing::info!(
                    "bevy_stdb: initial connection ready() returning true from cached result"
                );
                return true;
            }
        }

        tracing::info!("bevy_stdb: initial connection ready() polling task");
        let next_result = {
            let rx = state.rx.lock().unwrap_or_else(|e| e.into_inner());

            match rx.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    panic!("pending browser connection task disconnected before returning a result")
                }
            }
        };

        let Some(next_result) = next_result else {
            tracing::info!("bevy_stdb: initial connection ready() still pending");
            return false;
        };

        tracing::info!(
            "bevy_stdb: initial connection ready() received completed task result: success={}",
            next_result.is_ok()
        );

        let mut result = state.result.lock().unwrap_or_else(|e| e.into_inner());
        *result = Some(next_result);
        true
    }

    /// Establishes the initial connection and registers table handlers.
    fn finish(&self, app: &mut App) {
        tracing::info!("bevy_stdb: plugin finish() finalizing initial connection");
        let conn = {
            let state = app
                .world_mut()
                .remove_resource::<InitialConnectionState<C>>()
                .expect("InitialConnectionState should exist before plugin finish");

            state
                .result
                .into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .expect("plugin finish should only run after ready() returns true")
                .expect("Failed to establish initial connection")
        };

        let (table_registrar, background_driver) = {
            let config = app
                .world()
                .get_resource::<StdbConnectionConfig<C, M>>()
                .expect("StdbConnectionConfig should be inserted during plugin build");

            (
                config.table_registrar.clone(),
                config.background_driver.clone(),
            )
        };

        if let Some(register) = &table_registrar {
            let db = conn.db();
            register(&mut TableRegistrar::new_init(app), db);
        }

        if let Some(background_driver) = background_driver {
            tracing::info!("bevy_stdb: starting configured background driver");
            background_driver(conn.as_ref());
        }
        app.insert_resource(StdbConnection::new(conn));
        tracing::info!("bevy_stdb: initial connection resource inserted");
    }
}

fn watch_connected<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
>(
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

fn drive_connection_frame_tick<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
>(
    conn: Res<StdbConnection<C>>,
    config: Res<StdbConnectionConfig<C, M>>,
) {
    let Some(frame_tick) = config.frame_tick else {
        return;
    };

    let _ = frame_tick(conn.conn.as_ref());
}

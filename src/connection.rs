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
    app::AppExtStates,
    condition::in_state,
    state::{NextState, OnEnter, States},
};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, ConnectionId, DbConnectionBuilder, DbContext, Identity, Result,
};
#[cfg(feature = "browser")]
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, mpsc::Sender};

/// Internal startup status for the initial SpacetimeDB connection.
///
/// On native targets this begins in the ready state with the completed
/// initial connection result. On browser targets it begins pending and
/// transitions to ready once [`Plugin::ready`] observes the async result.
enum InitialConnectionStatus<C: DbContext + Send + Sync + 'static> {
    Ready(Result<Arc<C>>),
    #[cfg(feature = "browser")]
    Pending(Receiver<Result<Arc<C>>>),
}

/// Internal startup state for the initial SpacetimeDB connection.
///
/// This wraps the current startup status in interior mutability so
/// [`Plugin::ready`] can advance browser initialization before
/// [`Plugin::finish`] finalizes insertion of the live [`StdbConnection`] resource.
#[derive(Resource)]
struct InitialConnectionState<C: DbContext + Send + Sync + 'static> {
    status: Mutex<InitialConnectionStatus<C>>,
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

/// Internal connection driver configuration.
pub(crate) enum ConnectionDriver<C: DbContext + Send + Sync + 'static> {
    /// Drive the connection from the Bevy schedule each frame.
    FrameTick(fn(&C) -> Result<()>),
    /// Start connection processing in the background.
    Background(Arc<dyn Fn(&C) + Send + Sync>),
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
    /// The configured connection driver.
    pub driver: Option<ConnectionDriver<C>>,
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

impl<C> Clone for ConnectionDriver<C>
where
    C: DbContext + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        match self {
            Self::FrameTick(frame_tick) => Self::FrameTick(*frame_tick),
            Self::Background(background_driver) => Self::Background(background_driver.clone()),
        }
    }
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
            driver: self.driver.clone(),
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
    /// Internal helper to build the [`DbConnectionBuilder`] for this connection, shared across targets.
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

    /// Synchronously builds a SpacetimeDB connection from this config.
    ///
    /// The returned connection is not started automatically.
    #[cfg(not(feature = "browser"))]
    pub(crate) fn build_connection(&self) -> Result<Arc<C>> {
        self.connection_builder().build().map(Arc::new)
    }

    /// Asynchronously builds a SpacetimeDB connection from this config.
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
    /// The configured connection driver.
    pub driver: Option<ConnectionDriver<C>>,
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
        app.init_state::<StdbConnectionState>();

        let config = StdbConnectionConfig::<C, M> {
            module_name: self.module_name.clone(),
            uri: self.uri.clone(),
            token: self.token.clone(),
            driver: self.driver.clone(),
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

                wasm_bindgen_futures::spawn_local(async move {
                    let result = pending_config.build_connection().await;
                    let _ = tx.send(result);
                });

                InitialConnectionState::<C> {
                    status: Mutex::new(InitialConnectionStatus::Pending(rx)),
                }
            }

            #[cfg(not(feature = "browser"))]
            {
                InitialConnectionState::<C> {
                    status: Mutex::new(InitialConnectionStatus::Ready(config.build_connection())),
                }
            }
        };

        app.insert_resource(config);
        app.insert_resource(initial_connection_state);

        // Set our StdbConnectionState based on the connection state messages from SpacetimeDB.
        app.add_systems(PreUpdate, sync_connection_state);

        // Bind table callbacks when a new connection is established.
        app.add_systems(
            OnEnter(StdbConnectionState::Connected),
            on_connected_bind::<C, M>,
        );

        // We only need this system if frame tick driving is configured, which is a build time concern.
        if matches!(self.driver, Some(ConnectionDriver::FrameTick(_))) {
            app.add_systems(
                PreUpdate,
                drive_connection_frame_tick::<C, M>
                    .run_if(in_state(StdbConnectionState::Connected)),
            );
        }
    }

    #[cfg(feature = "browser")]
    fn ready(&self, app: &App) -> bool {
        let state = app
            .world()
            .get_resource::<InitialConnectionState<C>>()
            .expect("InitialConnectionState should be inserted during plugin build");

        let mut status = state.status.lock().unwrap_or_else(|e| e.into_inner());

        match &mut *status {
            InitialConnectionStatus::Ready(_) => true,
            InitialConnectionStatus::Pending(rx) => match rx.try_recv() {
                Ok(result) => {
                    *status = InitialConnectionStatus::Ready(result);
                    true
                }
                Err(TryRecvError::Empty) => false,
                Err(TryRecvError::Disconnected) => {
                    panic!("pending browser connection task disconnected before returning a result")
                }
            },
        }
    }

    /// Establishes the initial connection and registers table handlers.
    fn finish(&self, app: &mut App) {
        let state = app
            .world_mut()
            .remove_resource::<InitialConnectionState<C>>()
            .expect("InitialConnectionState should exist before plugin finish");

        let conn = match state.status.into_inner().unwrap_or_else(|e| e.into_inner()) {
            InitialConnectionStatus::Ready(result) => {
                result.expect("Failed to establish initial connection")
            }
            #[cfg(feature = "browser")]
            InitialConnectionStatus::Pending(_) => {
                panic!("plugin finish should only run after ready() returns true")
            }
        };

        let config = app
            .world()
            .get_resource::<StdbConnectionConfig<C, M>>()
            .expect("StdbConnectionConfig should be inserted during plugin build");

        let table_registrar = config.table_registrar.clone();
        let driver = config.driver.clone();

        if let Some(register) = table_registrar {
            let db = conn.db();
            register(&mut TableRegistrar::new_init(app), db);
        }

        if let Some(ConnectionDriver::Background(background_driver)) = driver {
            background_driver(conn.as_ref());
        }

        app.insert_resource(StdbConnection::new(conn));
    }
}

/// Synchronizes the connection state based on the connection state messages from SpacetimeDB.
/// Disconnected state takes precedence over connected state in ambiguous cases (multiple events per frame)
fn sync_connection_state(
    mut connected_msgs: ReadStdbConnectedMessage,
    mut disconnected_msgs: ReadStdbDisconnectedMessage,
    mut connection_error_msgs: ReadStdbConnectionErrorMessage,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
) {
    if connected_msgs.read().count() > 0 {
        next_state.set(StdbConnectionState::Connected);
    }
    if disconnected_msgs.read().count() > 0 {
        next_state.set(StdbConnectionState::Disconnected);
    }
    if connection_error_msgs.read().count() > 0 {
        next_state.set(StdbConnectionState::Disconnected);
    }
}

/// Bind the table callbacks when a new connection is established.
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

/// "tick" the connection frame, driving any pending operations. This is only used when the driver is `frame_tick`.
/// Uncommon use case, but its available when you want to have events processed at the bevy frame rate.
fn drive_connection_frame_tick<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
>(
    conn: Res<StdbConnection<C>>,
    config: Res<StdbConnectionConfig<C, M>>,
) {
    let ConnectionDriver::FrameTick(frame_tick) = config
        .driver
        .as_ref()
        .expect("frame tick system should only be added when a driver is configured")
    else {
        panic!("frame tick system should only be added when the frame tick driver is configured");
    };

    let _ = frame_tick(conn.conn.as_ref());
}

//! Connection state and resources.
//!
//! This module manages the active connection and its Bevy lifecycle integration.
use crate::{
    alias::{
        ReadStdbConnectedMessage, ReadStdbConnectionErrorMessage, ReadStdbDisconnectedMessage,
    },
    channel_bridge::{channel_sender, register_channel},
    message::{StdbConnectedMessage, StdbConnectionErrorMessage, StdbDisconnectedMessage},
    table::TableBindCallback,
};
use bevy_app::{App, Plugin, PreUpdate, Startup};
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
use crossbeam_channel::Sender;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, ConnectionId, DbConnectionBuilder, DbContext, Identity, Result,
};
#[cfg(feature = "browser")]
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

/// Internal runtime status for a requested SpacetimeDB connection build.
enum PendingConnectionStatus<C: DbContext + Send + Sync + 'static> {
    #[cfg(feature = "browser")]
    Pending(Receiver<Result<Arc<C>>>),
    Ready(Result<Arc<C>>),
}

/// Internal runtime state for an in-flight SpacetimeDB connection build.
#[derive(Resource)]
struct PendingConnectionState<C: DbContext + Send + Sync + 'static> {
    status: Mutex<PendingConnectionStatus<C>>,
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
    /// Whether startup should wait for an explicit connection request.
    pub delayed_connection: bool,
    /// Stored bind callbacks invoked for each active connection.
    pub table_bindings: Vec<Arc<TableBindCallback<C>>>,
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
            delayed_connection: self.delayed_connection,
            table_bindings: self.table_bindings.clone(),
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

/// Runtime controller for eager or delayed connection startup.
#[derive(Resource, Default)]
pub struct StdbConnectionController {
    requested: bool,
    token_override: Option<String>,
}

impl StdbConnectionController {
    /// Request that a connection be established using the configured token, if any.
    pub fn connect(&mut self) {
        self.requested = true;
        self.token_override = None;
    }

    /// Request that a connection be established using the supplied token.
    pub fn connect_with_token(&mut self, token: impl Into<String>) {
        self.requested = true;
        self.token_override = Some(token.into());
    }

    fn take_request(&mut self) -> Option<Option<String>> {
        if !self.requested {
            return None;
        }

        self.requested = false;
        Some(self.token_override.take())
    }
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
    /// Whether startup should wait for an explicit connection request.
    pub delayed_connection: bool,
    /// Stored bind callbacks invoked for each active connection.
    pub table_bindings: Vec<Arc<TableBindCallback<C>>>,
}

impl<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
> Plugin for StdbConnectionPlugin<C, M>
{
    /// Initializes connection state, resources, and lifecycle systems.
    fn build(&self, app: &mut App) {
        app.init_state::<StdbConnectionState>();

        register_channel::<StdbConnectedMessage>(app);
        register_channel::<StdbDisconnectedMessage>(app);
        register_channel::<StdbConnectionErrorMessage>(app);

        let world = app.world();
        let config = StdbConnectionConfig::<C, M> {
            module_name: self.module_name.clone(),
            uri: self.uri.clone(),
            token: self.token.clone(),
            driver: self.driver.clone(),
            compression: self.compression,
            delayed_connection: self.delayed_connection,
            table_bindings: self.table_bindings.clone(),
            connected_tx: channel_sender::<StdbConnectedMessage>(world),
            disconnected_tx: channel_sender::<StdbDisconnectedMessage>(world),
            error_tx: channel_sender::<StdbConnectionErrorMessage>(world),
        };

        app.insert_resource(config);
        app.insert_resource(StdbConnectionController::default());

        if !self.delayed_connection {
            app.add_systems(Startup, request_initial_connection);
        }

        // Set our StdbConnectionState based on the connection state messages from SpacetimeDB.
        app.add_systems(PreUpdate, sync_connection_state);

        // Start a connection whenever it is requested.
        app.add_systems(
            PreUpdate,
            start_requested_connection::<C, M>.run_if(in_state(StdbConnectionState::Uninitialized)),
        );

        // Poll any in-flight browser connection build.
        #[cfg(feature = "browser")]
        app.add_systems(PreUpdate, poll_pending_connection::<C, M>);

        // Finalize a completed connection build on all targets.
        app.add_systems(PreUpdate, finalize_pending_connection::<C, M>);

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
}

/// Request an eager connection during startup unless delayed connection is enabled.
fn request_initial_connection(mut controller: ResMut<StdbConnectionController>) {
    controller.connect();
}

/// Start building a connection when requested at runtime.
fn start_requested_connection<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    config: Res<StdbConnectionConfig<C, M>>,
    mut controller: ResMut<StdbConnectionController>,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
    mut commands: bevy_ecs::system::Commands,
) {
    let Some(token_override) = controller.take_request() else {
        return;
    };

    let mut connect_config = config.clone();
    if let Some(token) = token_override {
        connect_config.token = Some(token);
    }

    #[cfg(feature = "browser")]
    {
        let (tx, rx) = channel();

        wasm_bindgen_futures::spawn_local(async move {
            let result = connect_config.build_connection().await;
            let _ = tx.send(result);
        });

        commands.insert_resource(PendingConnectionState::<C> {
            status: Mutex::new(PendingConnectionStatus::Pending(rx)),
        });
    }

    #[cfg(not(feature = "browser"))]
    {
        commands.insert_resource(PendingConnectionState::<C> {
            status: Mutex::new(PendingConnectionStatus::Ready(
                connect_config.build_connection(),
            )),
        });
    }

    next_state.set(StdbConnectionState::Reconnecting);
}

#[cfg(feature = "browser")]
fn poll_pending_connection<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    state: Res<PendingConnectionState<C>>,
) {
    let mut status = state.status.lock().unwrap_or_else(|e| e.into_inner());

    match &mut *status {
        PendingConnectionStatus::Ready(_) => {}
        PendingConnectionStatus::Pending(rx) => match rx.try_recv() {
            Ok(result) => {
                *status = PendingConnectionStatus::Ready(result);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                panic!("pending browser connection task disconnected before returning a result")
            }
        },
    }
}

/// Finalize a completed connection build by inserting the active connection resource.
fn finalize_pending_connection<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    mut commands: bevy_ecs::system::Commands,
    pending: Option<Res<PendingConnectionState<C>>>,
    config: Res<StdbConnectionConfig<C, M>>,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
) {
    let Some(pending) = pending else {
        return;
    };

    let ready_result = {
        let mut status = pending.status.lock().unwrap_or_else(|e| e.into_inner());
        match &mut *status {
            #[cfg(feature = "browser")]
            PendingConnectionStatus::Pending(_) => return,
            PendingConnectionStatus::Ready(result) => result.clone(),
        }
    };

    commands.remove_resource::<PendingConnectionState<C>>();

    match ready_result {
        Ok(conn) => {
            if let Some(ConnectionDriver::Background(background_driver)) = config.driver.clone() {
                background_driver(conn.as_ref());
            }

            commands.insert_resource(StdbConnection::new(conn));
        }
        Err(_) => {
            next_state.set(StdbConnectionState::Disconnected);
        }
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
    for bind in &config.table_bindings {
        bind(&*world, db);
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

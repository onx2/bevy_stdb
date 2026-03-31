//! Connection state and resources.
//!
//! This module manages the active SpacetimeDB connection and its Bevy
//! lifecycle integration.
//!
//! Connection setup is handled entirely through runtime systems rather than
//! plugin `ready()` hooks. This allows the same implementation to support:
//!
//! - native and browser targets
//! - eager startup connections
//! - delayed/manual connection through [`StdbConnectionController`]
//! - reconnect flows that reuse the most recently stored token
//!
//! The general flow is:
//!
//! - insert [`StdbConnectionConfig`] and [`StdbConnectionController`] during
//!   plugin build
//! - request connection eagerly during startup or later at runtime
//! - build the connection through native or browser-specific code paths
//! - insert [`StdbConnection`] once the build succeeds
//! - bind deferred table callbacks after the SDK reports the connection as
//!   connected
//!
//! Browser connection attempts and browser reconnect attempts both use the same
//! pending-result resource so runtime polling and finalization behave the same
//! way in both flows.
use crate::{
    alias::{
        ReadStdbConnectedMessage, ReadStdbConnectionErrorMessage, ReadStdbDisconnectedMessage,
    },
    channel_bridge::{channel_sender, register_channel},
    message::{StdbConnectedMessage, StdbConnectionErrorMessage, StdbDisconnectedMessage},
    table::TableBindCallback,
};
use bevy_app::{App, Plugin, PreStartup, PreUpdate};
use bevy_ecs::{
    resource::Resource,
    schedule::{IntoScheduleConfigs, SystemCondition},
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

/// Internal runtime result type for a completed SpacetimeDB connection build.
pub(crate) type ConnectionBuildResult<C> = Result<Arc<C>>;

/// Internal runtime status for an in-flight SpacetimeDB connection build.
pub(crate) enum PendingConnectionStatus<C: DbContext + Send + Sync + 'static> {
    #[cfg(feature = "browser")]
    Pending(Receiver<ConnectionBuildResult<C>>),
    Ready(ConnectionBuildResult<C>),
}

/// Internal runtime state for an in-flight SpacetimeDB connection build.
#[derive(Resource)]
pub(crate) struct PendingConnectionState<C: DbContext + Send + Sync + 'static> {
    pub(crate) status: Mutex<PendingConnectionStatus<C>>,
}

/// Begin a browser connection build and store its pending result resource.
#[cfg(feature = "browser")]
pub(crate) fn begin_browser_connection_build<C, F>(
    commands: &mut bevy_ecs::system::Commands,
    build: F,
) where
    C: DbContext + Send + Sync + 'static,
    F: 'static + std::future::Future<Output = ConnectionBuildResult<C>>,
{
    let (tx, rx) = channel();

    wasm_bindgen_futures::spawn_local(async move {
        let result = build.await;
        let _ = tx.send(result);
    });

    commands.insert_resource(PendingConnectionState::<C> {
        status: Mutex::new(PendingConnectionStatus::Pending(rx)),
    });
}

/// Poll an in-flight browser connection build until it produces a result.
#[cfg(feature = "browser")]
pub(crate) fn poll_browser_connection_build<C>(world: &mut bevy_ecs::world::World)
where
    C: DbContext + Send + Sync + 'static,
{
    let Some(state) = world.get_resource::<PendingConnectionState<C>>() else {
        return;
    };

    let mut status = state.status.lock().unwrap_or_else(|e| e.into_inner());

    match &mut *status {
        PendingConnectionStatus::Ready(_) => {}
        PendingConnectionStatus::Pending(rx) => match rx.try_recv() {
            Ok(result) => {
                *status = PendingConnectionStatus::Ready(result);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                *status = PendingConnectionStatus::Ready(Err(spacetimedb_sdk::Error::Disconnected));
            }
        },
    }
}

/// Take a completed pending connection build result if one is ready.
pub(crate) fn take_pending_connection_result<C>(
    world: &mut bevy_ecs::world::World,
) -> Option<ConnectionBuildResult<C>>
where
    C: DbContext + Send + Sync + 'static,
{
    let ready_result = {
        let pending = world.get_resource::<PendingConnectionState<C>>()?;
        let mut status = pending.status.lock().unwrap_or_else(|e| e.into_inner());
        match &mut *status {
            #[cfg(feature = "browser")]
            PendingConnectionStatus::Pending(_) => return None,
            PendingConnectionStatus::Ready(result) => result.clone(),
        }
    };

    world.remove_resource::<PendingConnectionState<C>>();
    Some(ready_result)
}

/// Lifecycle [`States`] for the active SpacetimeDB connection.
///
/// `Connected` and `Disconnected` are driven by SDK lifecycle messages, while
/// `Reconnecting` and `Exhausted` are policy-oriented states managed by the
/// reconnect subsystem.
#[derive(States, Debug, Default, Clone, PartialEq, Eq, Hash)]
pub enum StdbConnectionState {
    /// No connection attempt has been started yet.
    #[default]
    Uninitialized,

    /// An initial or manually requested connection attempt is in progress.
    Connecting,

    /// The SDK has reported that the connection is active.
    Connected,

    /// No active connection is available.
    ///
    /// This state is entered after a disconnect or a failed connection attempt.
    Disconnected,

    /// An automatic reconnect attempt is in progress.
    Reconnecting,

    /// Reconnect attempts have been exhausted.
    ///
    /// No further connection attempts will be made.
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
///
/// This resource is inserted once a connection build succeeds. When delayed
/// connection is enabled, or before the initial/manual connection attempt has
/// completed, this resource will not yet exist.
#[derive(Resource)]
pub struct StdbConnection<T: DbContext + 'static> {
    /// The underlying connection context.
    conn: Arc<T>,
}

/// Runtime controller for eager or delayed connection startup.
///
/// This resource is always inserted during plugin build. In eager mode, the
/// plugin requests an initial connection automatically during startup. In
/// delayed mode, application code can request connection later by calling
/// [`Self::connect`] or [`Self::connect_with_token`].
///
/// If a token is provided through [`Self::connect_with_token`], it becomes the
/// most recently stored token and will be reused by future reconnect attempts.
#[derive(Resource, Default)]
pub struct StdbConnectionController {
    requested: bool,
    token_override: Option<String>,
}

impl StdbConnectionController {
    /// Request that a connection be established using the currently stored
    /// token, if any.
    ///
    /// If a connection is already active or a connection attempt is already in
    /// progress, the request is dropped.
    pub fn connect(&mut self) {
        self.requested = true;
        self.token_override = None;
    }

    /// Request that a connection be established using the supplied token.
    ///
    /// The supplied token becomes the most recently stored token and will be
    /// reused by future reconnect attempts after the request is processed.
    ///
    /// If a connection is already active or a connection attempt is already in
    /// progress, the request is dropped.
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

    fn clear_request(&mut self) {
        self.requested = false;
        self.token_override = None;
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

    /// Returns `true` if the underlying SDK connection is currently active.
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
///
/// This plugin installs the resources and systems needed to support eager or
/// delayed startup, native or browser connection building, and deferred table
/// binding after connection.
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
            app.add_systems(PreStartup, request_initial_connection);
        }

        // Set our StdbConnectionState based on the connection state messages from SpacetimeDB.
        app.add_systems(PreUpdate, sync_connection_state);

        // Start a connection whenever it is requested.
        app.add_systems(
            PreUpdate,
            start_requested_connection::<C, M>.run_if(
                in_state(StdbConnectionState::Uninitialized)
                    .or(in_state(StdbConnectionState::Connecting)),
            ),
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

/// Request an eager connection during startup unless delayed connection is
/// enabled.
fn request_initial_connection(mut controller: ResMut<StdbConnectionController>) {
    controller.connect();
}

/// Start building a connection when requested at runtime.
///
/// This system consumes the pending request from
/// [`StdbConnectionController`], persists any token override into the stored
/// connection config, and then begins a native or browser-specific connection
/// build.
///
/// Requests are dropped if a connection is already active or another connection
/// attempt is already pending.
fn start_requested_connection<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    mut config: ResMut<StdbConnectionConfig<C, M>>,
    mut controller: ResMut<StdbConnectionController>,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
    active_connection: Option<Res<StdbConnection<C>>>,
    pending_connection: Option<Res<PendingConnectionState<C>>>,
    mut commands: bevy_ecs::system::Commands,
) {
    let Some(token_override) = controller.take_request() else {
        return;
    };

    if active_connection.is_some() || pending_connection.is_some() {
        controller.clear_request();
        return;
    }

    if let Some(token) = token_override {
        config.token = Some(token);
    }

    let connect_config = config.clone();

    #[cfg(feature = "browser")]
    {
        begin_browser_connection_build::<C, _>(&mut commands, async move {
            connect_config.build_connection().await
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

    next_state.set(StdbConnectionState::Connecting);
}

#[cfg(feature = "browser")]
/// Poll any in-flight browser connection build until it produces a result.
fn poll_pending_connection<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut bevy_ecs::world::World,
) {
    poll_browser_connection_build::<C>(world);
}

/// Inserts an active connection resource and starts the configured driver.
///
/// This helper is shared by the initial/manual connection flow and reconnect
/// success handling.
pub(crate) fn activate_connection<C>(
    world: &mut bevy_ecs::world::World,
    conn: Arc<C>,
    driver: Option<ConnectionDriver<C>>,
) where
    C: DbContext + Send + Sync + 'static,
{
    if let Some(ConnectionDriver::Background(background_driver)) = driver {
        background_driver(conn.as_ref());
    }

    world.insert_resource(StdbConnection::new(conn));
}

/// Finalize a completed connection build.
///
/// On success this inserts [`StdbConnection`] and starts the configured
/// background driver if needed. On failure this transitions the lifecycle state
/// to [`StdbConnectionState::Disconnected`], allowing reconnect policy to take
/// over if configured.
fn finalize_pending_connection<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut bevy_ecs::world::World,
) {
    let Some(ready_result) = take_pending_connection_result::<C>(world) else {
        return;
    };

    match ready_result {
        Ok(conn) => {
            let driver = world
                .get_resource::<StdbConnectionConfig<C, M>>()
                .expect("StdbConnectionConfig should exist before finalizing a connection")
                .driver
                .clone();
            activate_connection(world, conn, driver);
        }
        Err(_) => {
            world
                .get_resource_mut::<NextState<StdbConnectionState>>()
                .expect(
                    "NextState<StdbConnectionState> should exist before finalizing a connection",
                )
                .set(StdbConnectionState::Disconnected);
        }
    }
}

/// Synchronize [`StdbConnectionState`] from SDK lifecycle messages.
///
/// `Disconnected` takes precedence over `Connected` when multiple lifecycle
/// messages are observed in the same frame.
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

/// Bind all deferred table callbacks after a connection becomes active.
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

/// Tick the active connection once per frame when the frame driver is in use.
///
/// This is only added when [`ConnectionDriver::FrameTick`] is configured.
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

//! Connection state and lifecycle for SpacetimeDB.
//!
//! Manages the active connection, lifecycle states, and related resources.
use crate::{
    alias::{
        ReadStdbConnectedMessage, ReadStdbConnectionErrorMessage, ReadStdbDisconnectedMessage,
    },
    auth::StdbAuthTarget,
    channel_bridge::{channel_sender, register_channel},
    message::{
        ConnectionBuildFinishedMessage, RequestStdbConnectionMessage, StdbConnectedMessage,
        StdbConnectionErrorMessage, StdbDisconnectedMessage,
    },
    set::StdbSet,
};

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{
    Commands, IntoScheduleConfigs, Messages, Res, ResMut, Resource, World, not,
};
use bevy_state::prelude::{AppExtStates, NextState, States, in_state};
use crossbeam_channel::Sender;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, ConnectionId, DbConnectionBuilder, DbContext, Identity, Result,
};
use std::sync::Arc;

/// Lifecycle [`States`] for the active SpacetimeDB connection.
///
/// `Connected` and `Disconnected` are driven by SDK lifecycle messages, while
/// `Exhausted` is a policy-oriented state managed by the reconnect subsystem.
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

    /// Reconnect attempts have been exhausted.
    ///
    /// No further connection attempts will be made.
    Exhausted,
}

/// Internal connection driver configuration.
pub(crate) enum ConnectionDriver<C: DbContext + Send + Sync + 'static> {
    /// Drives the connection from the Bevy schedule each frame.
    FrameTick(fn(&C) -> Result<()>),
    /// Starts connection processing in the background.
    Background(Arc<dyn Fn(&C) + Send + Sync>),
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

/// Runtime configuration for the active SpacetimeDB connection.
#[derive(Resource)]
pub(crate) struct StdbConnectionConfig<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    /// The remote module/database name.
    module_name: String,
    /// The URI of the SpacetimeDB host.
    uri: String,
    /// Optional authentication token.
    token: Option<String>,
    /// The configured connection driver.
    driver: Option<ConnectionDriver<C>>,
    /// Compression configuration for the connection.
    compression: Compression,
    /// Sender used by the SpacetimeDB on-connect callback.
    connected_tx: Sender<StdbConnectedMessage>,
    /// Sender used by the SpacetimeDB on-disconnect callback.
    disconnected_tx: Sender<StdbDisconnectedMessage>,
    /// Sender used by the SpacetimeDB on-connect-error callback.
    error_tx: Sender<StdbConnectionErrorMessage>,
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
    /// Produces a configured [`DbConnectionBuilder`] for this connection.
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
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn build_connection(&self) -> Result<Arc<C>> {
        self.connection_builder().build().map(Arc::new)
    }

    /// Asynchronously builds a SpacetimeDB connection from this config.
    ///
    /// The returned connection is not started automatically.
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn build_connection(&self) -> Result<Arc<C>> {
        self.connection_builder().build().await.map(Arc::new)
    }
}

/// Active SpacetimeDB connection [`Resource`].
///
/// Inserted once a connection build succeeds. Will not exist while delayed
/// connection is enabled or before the initial connection attempt completes.
#[derive(Resource)]
pub struct StdbConnection<T: DbContext + 'static> {
    /// The underlying connection context.
    conn: Arc<T>,
}

impl<T: DbContext> StdbConnection<T> {
    /// Wraps an existing shared connection.
    fn new(conn: Arc<T>) -> Self {
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
/// Installs the resources and systems for eager or delayed startup, native or
/// browser connection building, and deferred table binding.
pub(crate) struct StdbConnectionPlugin<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    /// The remote module/database name.
    pub module_name: String,
    /// The URI of the SpacetimeDB host.
    pub uri: String,
    /// The configured connection driver.
    pub driver: Option<ConnectionDriver<C>>,
    /// Compression configuration for the connection.
    pub compression: Compression,
}

impl<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
> Plugin for StdbConnectionPlugin<C, M>
{
    /// Initializes connection state, resources, and lifecycle systems.
    fn build(&self, app: &mut App) {
        app.init_state::<StdbConnectionState>();
        app.add_message::<RequestStdbConnectionMessage>();

        register_channel::<StdbConnectedMessage>(app);
        register_channel::<StdbDisconnectedMessage>(app);
        register_channel::<StdbConnectionErrorMessage>(app);

        #[cfg(target_arch = "wasm32")]
        register_channel::<ConnectionBuildFinishedMessage<C>>(app);

        #[cfg(not(target_arch = "wasm32"))]
        app.add_message::<ConnectionBuildFinishedMessage<C>>();

        let world = app.world();
        app.insert_resource(StdbConnectionConfig::<C, M> {
            module_name: self.module_name.clone(),
            uri: self.uri.clone(),
            token: None,
            driver: self.driver.clone(),
            compression: self.compression,
            connected_tx: channel_sender::<StdbConnectedMessage>(world),
            disconnected_tx: channel_sender::<StdbDisconnectedMessage>(world),
            error_tx: channel_sender::<StdbConnectionErrorMessage>(world),
        });

        // Sync connection state from SDK lifecycle messages.
        app.add_systems(
            PreUpdate,
            sync_connection_state::<C>.in_set(StdbSet::StateSync),
        );

        app.add_systems(
            PreUpdate,
            handle_connection_request::<C, M>
                .in_set(StdbSet::Connection)
                .run_if(not(in_state(StdbConnectionState::Connected)))
                .run_if(not(in_state(StdbConnectionState::Connecting))),
        );

        app.add_systems(
            PreUpdate,
            finalize_pending_connection::<C, M>.in_set(StdbSet::Connection),
        );

        // Only added when frame-tick driving is configured.
        if matches!(self.driver, Some(ConnectionDriver::FrameTick(_))) {
            app.add_systems(
                PreUpdate,
                (|conn: Res<StdbConnection<C>>, config: Res<StdbConnectionConfig<C, M>>| {
                    let Some(ConnectionDriver::FrameTick(frame_tick)) = config.driver.as_ref() else {
                        panic!("frame tick system should only be added when the frame tick driver is configured");
                    };

                    let _ = frame_tick(conn.conn.as_ref());
                })
                .in_set(StdbSet::Connection)
                .run_if(in_state(StdbConnectionState::Connected)),
            );
        }
    }
}

/// Initiates a connection build from a connection request message.
///
/// Requests can override the current connection configuration and
/// while an active connection exists, this will clear any pending requests.
fn handle_connection_request<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    // Ignore requests while currently connected
    if world.get_resource::<StdbConnection<C>>().is_some() {
        return world
            .resource_mut::<Messages<RequestStdbConnectionMessage>>()
            .clear();
    }

    // Use the most recent request for the connection attempt
    let Some(request) = world
        .resource_mut::<Messages<RequestStdbConnectionMessage>>()
        .drain()
        .last()
    else {
        return;
    };

    world
        .resource_mut::<NextState<StdbConnectionState>>()
        .set(StdbConnectionState::Connecting);

    #[cfg(not(target_arch = "wasm32"))]
    {
        // Get the current configuration and override if requested
        let connect_config = {
            let mut config = world.resource_mut::<StdbConnectionConfig<C, M>>();
            if let Some(auth_target) = request.auth_target {
                config.token = match auth_target {
                    #[cfg(feature = "auth-oidc")]
                    StdbAuthTarget::Oidc(opts) => None, // TODO: authenticate via OIDC... blocking
                    #[cfg(feature = "auth-steam")]
                    StdbAuthTarget::Steam(opts) => None, // TODO: authenticate via Steam... blocking
                    StdbAuthTarget::Token(token) => Some(token),
                };
            };
            // TODO - this is now auth_target
            // config.token = request.token.or(config.token.take());
            config.uri = request.uri.unwrap_or(config.uri.clone());
            config.module_name = request.module_name.unwrap_or(config.module_name.clone());
            config.clone()
        };
        world.write_message(ConnectionBuildFinishedMessage {
            result: connect_config.build_connection(),
        });
    }

    #[cfg(target_arch = "wasm32")]
    {
        let sender = channel_sender::<ConnectionBuildFinishedMessage<C>>(world);
        js_sys::futures::spawn_local(async move {
            // Get the current configuration and override if requested
            let connect_config = {
                let mut config = world.resource_mut::<StdbConnectionConfig<C, M>>();
                // TODO - this is now auth_target
                // config.token = request.token.or(config.token.take());
                config.uri = request.uri.unwrap_or(config.uri.clone());
                config.module_name = request.module_name.unwrap_or(config.module_name.clone());
                config.clone()
            };

            let _ = sender.send(ConnectionBuildFinishedMessage {
                result: connect_config.build_connection().await,
            });
        });
    }
}

/// Completes a pending connection build and transitions [`StdbConnectionState`] accordingly.
fn finalize_pending_connection<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    let finished_msgs: Vec<ConnectionBuildFinishedMessage<C>> = {
        let mut messages = world.resource_mut::<Messages<ConnectionBuildFinishedMessage<C>>>();
        messages.drain().collect()
    };

    for msg in finished_msgs {
        match msg.result {
            Ok(conn) => {
                let driver = world
                    .get_resource::<StdbConnectionConfig<C, M>>()
                    .expect("StdbConnectionConfig should exist when activating a connection")
                    .driver
                    .clone();

                if let Some(ConnectionDriver::Background(background_driver)) = driver {
                    background_driver(conn.as_ref());
                }

                world.insert_resource(StdbConnection::new(conn));
            }
            Err(_) => {
                world
                    .resource_mut::<NextState<StdbConnectionState>>()
                    .set(StdbConnectionState::Disconnected);
            }
        }
    }
}

/// Synchronizes [`StdbConnectionState`] from SDK lifecycle messages.
///
/// [`StdbConnectionState::Disconnected`] takes precedence when multiple
/// lifecycle messages arrive in the same frame.
fn sync_connection_state<C: DbContext + Send + Sync + 'static>(
    mut connected_msgs: ReadStdbConnectedMessage,
    mut disconnected_msgs: ReadStdbDisconnectedMessage,
    mut connection_error_msgs: ReadStdbConnectionErrorMessage,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
    mut commands: Commands,
) {
    if connected_msgs.read().count() > 0 {
        next_state.set(StdbConnectionState::Connected);
    }
    if disconnected_msgs.read().count() > 0 {
        commands.remove_resource::<StdbConnection<C>>();
        next_state.set(StdbConnectionState::Disconnected);
    }
    if connection_error_msgs.read().count() > 0 {
        commands.remove_resource::<StdbConnection<C>>();
        next_state.set(StdbConnectionState::Disconnected);
    }
}

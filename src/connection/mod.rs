//! Connection state and lifecycle for SpacetimeDB.
//!
//! Manages the active connection, lifecycle states, and related resources.

mod reconnect;

use crate::{
    alias::{ReadStdbConnectErrorMessage, ReadStdbConnectedMessage, ReadStdbDisconnectedMessage},
    channel_bridge::{channel_sender, register_channel},
    message::{
        DisconnectIntent, StdbConnectErrorMessage, StdbConnectedMessage, StdbDisconnectedMessage,
        StdbDriverErrorMessage,
    },
    set::StdbSet,
    table::bind_tables,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{
    Commands, IntoScheduleConfigs, MessageWriter, Res, Resource, World, resource_exists,
};
use bevy_tasks::{IoTaskPool, Task, block_on, poll_once};
use crossbeam_channel::Sender;
pub use reconnect::StdbReconnectOptions;
pub(crate) use reconnect::{ReconnectPlugin, end_reconnect_cycle};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, ConnectionId, DbConnectionBuilder, DbContext, Identity, Result,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Stores the in-flight task for a pending connection attempt.
#[derive(Resource)]
pub(crate) struct PendingConnection<C: DbContext + Send + Sync + 'static>(
    pub(crate) Task<Result<StdbConnection<C>>>,
);

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
    pub(crate) database_name: String,
    /// The URI of the SpacetimeDB host.
    pub(crate) uri: String,
    /// Optional authentication token.
    pub(crate) token: Option<String>,
    /// The configured connection driver.
    driver: ConnectionDriver<C>,
    /// Compression configuration for the connection.
    compression: Compression,
    /// Sender used by the SpacetimeDB on-connect callback.
    connected_tx: Sender<StdbConnectedMessage>,
    /// Sender used by the SpacetimeDB on-disconnect callback.
    disconnected_tx: Sender<StdbDisconnectedMessage>,
    /// Sender used by the SpacetimeDB on-connection error callback.
    connect_error_tx: Sender<StdbConnectErrorMessage>,
}

impl<C, M> Clone for StdbConnectionConfig<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    fn clone(&self) -> Self {
        Self {
            database_name: self.database_name.clone(),
            uri: self.uri.clone(),
            token: self.token.clone(),
            driver: self.driver.clone(),
            compression: self.compression,
            connected_tx: self.connected_tx.clone(),
            disconnected_tx: self.disconnected_tx.clone(),
            connect_error_tx: self.connect_error_tx.clone(),
        }
    }
}

impl<C, M> StdbConnectionConfig<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    /// Produces a configured [`DbConnectionBuilder`] for this connection.
    ///
    /// `disconnect_requested` is the flag [`StdbConnection::disconnect`] raises. The SDK reports
    /// a connection the server dropped exactly as it reports one this client closed -- a
    /// disconnect with no error -- so the flag is the only thing that tells them apart.
    fn connection_builder(&self, disconnect_requested: Arc<AtomicBool>) -> DbConnectionBuilder<M> {
        let connected_tx = self.connected_tx.clone();
        let disconnected_tx = self.disconnected_tx.clone();
        let connect_error_tx = self.connect_error_tx.clone();

        DbConnectionBuilder::<M>::new()
            .with_database_name(self.database_name.clone())
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
                let result = if disconnect_requested.swap(false, Ordering::AcqRel) {
                    Ok(DisconnectIntent::Requested)
                } else {
                    err.map_or(Ok(DisconnectIntent::Lost), Err)
                };
                let _ = disconnected_tx.send(StdbDisconnectedMessage { result });
            })
            .on_connect_error(move |_ctx, err| {
                let _ = connect_error_tx.send(StdbConnectErrorMessage { err });
            })
    }

    /// Builds a SpacetimeDB connection from this config.
    ///
    /// The returned connection is not started automatically.
    pub(crate) async fn build_connection(&self) -> Result<StdbConnection<C>> {
        let disconnect_requested = Arc::new(AtomicBool::new(false));
        let builder = self.connection_builder(Arc::clone(&disconnect_requested));

        #[cfg(not(feature = "browser"))]
        let conn = builder.build()?;
        #[cfg(feature = "browser")]
        let conn = builder.build().await?;

        Ok(StdbConnection {
            conn: Arc::new(conn),
            disconnect_requested,
        })
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
    /// Raised by [`Self::disconnect`] and read by this connection's `on_disconnect` callback.
    disconnect_requested: Arc<AtomicBool>,
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
    ///
    /// The resulting disconnect message reports [`DisconnectIntent::Requested`], which
    /// the reconnect cycle leaves alone.
    pub fn disconnect(&self) -> Result<()> {
        // Kept raised even if the SDK refuses: it only refuses a connection that is already
        // closing, and if that close was an earlier request, clearing the flag would report it
        // as lost and retry it.
        self.disconnect_requested.store(true, Ordering::Release);
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
    pub database_name: String,
    /// The URI of the SpacetimeDB host.
    pub uri: String,
    /// The authentication token for the connection.
    pub token: Option<String>,
    /// Starts the initial connection when the plugin is built.
    pub eager_connection: bool,
    /// The configured connection driver.
    pub driver: ConnectionDriver<C>,
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
        register_channel::<StdbConnectedMessage>(app);
        register_channel::<StdbDisconnectedMessage>(app);
        register_channel::<StdbConnectErrorMessage>(app);

        let world = app.world();
        app.insert_resource(StdbConnectionConfig::<C, M> {
            database_name: self.database_name.clone(),
            uri: self.uri.clone(),
            token: self.token.clone(),
            driver: self.driver.clone(),
            compression: self.compression,
            connected_tx: channel_sender::<StdbConnectedMessage>(world),
            disconnected_tx: channel_sender::<StdbDisconnectedMessage>(world),
            connect_error_tx: channel_sender::<StdbConnectErrorMessage>(world),
        });

        app.add_systems(
            PreUpdate,
            sync_connection_resource::<C>.in_set(StdbSet::StateSync),
        );

        app.add_systems(
            PreUpdate,
            poll_pending_connection::<C, M>
                .run_if(resource_exists::<PendingConnection<C>>)
                .in_set(StdbSet::Connection),
        );

        app.add_message::<StdbDriverErrorMessage>();
        if let ConnectionDriver::FrameTick(frame_tick) = self.driver {
            app.add_systems(
                PreUpdate,
                (move |conn: Res<StdbConnection<C>>,
                       mut errors: MessageWriter<StdbDriverErrorMessage>| {
                    match frame_tick(conn.conn.as_ref()) {
                        Ok(()) => {}
                        // An error that ended the connection was already reported by the SDK
                        // callback it ran -- `on_disconnect`, or `on_connect_error` before the
                        // handshake -- and `frame_tick` returns it again.
                        Err(_) if !conn.is_active() => {}
                        Err(err) => {
                            errors.write(StdbDriverErrorMessage { err });
                        }
                    }
                })
                .in_set(StdbSet::Drive)
                .run_if(resource_exists::<StdbConnection<C>>),
            );
        }

        if self.eager_connection {
            let config = app.world().resource::<StdbConnectionConfig<C, M>>().clone();
            let task = IoTaskPool::get().spawn(async move { config.build_connection().await });
            app.insert_resource(PendingConnection::<C>(task));
        }
    }
}

/// Polls the pending connection attempt, activating the connection once it resolves.
fn poll_pending_connection<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    // Polled in place: a resource is a component on its own entity, so taking it out and putting
    // it back every frame would move it between archetypes twice for nothing.
    let Some(mut pending) = world.get_resource_mut::<PendingConnection<C>>() else {
        return;
    };
    let Some(result) = block_on(poll_once(&mut pending.0)) else {
        return;
    };
    world.remove_resource::<PendingConnection<C>>();

    match result {
        Ok(conn) => {
            // Bound here rather than by a system watching for the resource: a row callback must
            // exist before anything can deliver a row, so the tables are bound before the
            // driver starts and before any subscription is applied.
            bind_tables::<C, M>(world, conn.db());

            let config = world.resource::<StdbConnectionConfig<C, M>>();
            if let ConnectionDriver::Background(background_driver) = &config.driver {
                background_driver(conn.conn.as_ref());
            }

            // Removed rather than overwritten, so subscriptions see the new connection as added.
            if let Some(prev_conn) = world.remove_resource::<StdbConnection<C>>() {
                let _ = prev_conn.disconnect();
            }
            world.insert_resource(conn);
        }
        Err(err) => {
            world.write_message(StdbConnectErrorMessage { err });
        }
    }
}

/// Removes the [`StdbConnection`] once the SDK reports it closed.
///
/// A connection that closes before its initial connection message is reported as a connect
/// error, not a disconnect, so both are watched; left in place, a dead connection would pause the
/// reconnect countdown and turn every [`StdbCommands::connect`](crate::prelude::StdbCommands::connect)
/// into a no-op.
fn sync_connection_resource<C: DbContext + Send + Sync + 'static>(
    mut connected: ReadStdbConnectedMessage,
    mut disconnected: ReadStdbDisconnectedMessage,
    mut connect_errors: ReadStdbConnectErrorMessage,
    conn: Option<Res<StdbConnection<C>>>,
    mut commands: Commands,
) {
    let reported =
        connected.read().count() + disconnected.read().count() + connect_errors.read().count();
    if reported > 0 && conn.is_some_and(|conn| !conn.is_active()) {
        commands.remove_resource::<StdbConnection<C>>();
    }
}

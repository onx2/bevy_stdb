//! Connection state and lifecycle for SpacetimeDB.
//!
//! Manages the active connection, lifecycle states, and related resources.

<<<<<<< HEAD
pub(crate) mod reconnect;
=======
pub mod reconnect;
>>>>>>> origin/main

use crate::{
    alias::{ReadStdbConnectedMessage, ReadStdbDisconnectedMessage},
    channel_bridge::{channel_sender, register_channel},
<<<<<<< HEAD
    message::{StdbConnectedMessage, StdbDisconnectedMessage},
=======
    message::{StdbConnectErrorMessage, StdbConnectedMessage, StdbDisconnectedMessage},
>>>>>>> origin/main
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{Commands, IntoScheduleConfigs, Res, Resource, World, resource_exists};
<<<<<<< HEAD
use bevy_log::error;
=======
>>>>>>> origin/main
use bevy_tasks::{Task, block_on, poll_once};
use crossbeam_channel::Sender;
pub(crate) use reconnect::ReconnectPlugin;
pub use reconnect::StdbReconnectOptions;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, ConnectionId, DbConnectionBuilder, DbContext, Identity, Result,
};
use std::sync::Arc;

/// Stores the in-flight task for a pending connection attempt.
#[derive(Resource)]
pub(crate) struct PendingConnection<C: DbContext + Send + Sync + 'static>(
    pub(crate) Task<Result<Arc<C>>>,
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
    pub(crate) module_name: String,
    /// The URI of the SpacetimeDB host.
    pub(crate) uri: String,
    /// Optional authentication token.
    pub(crate) token: Option<String>,
<<<<<<< HEAD
    /// Optional client ID for authentication.
    client_id: Option<String>,
    /// Optional ID token returned by the OIDC provider.
    id_token: Option<String>,
=======
>>>>>>> origin/main
    /// The configured connection driver.
    driver: Option<ConnectionDriver<C>>,
    /// Compression configuration for the connection.
    compression: Compression,
    /// Sender used by the SpacetimeDB on-connect callback.
    connected_tx: Sender<StdbConnectedMessage>,
    /// Sender used by the SpacetimeDB on-disconnect callback.
    disconnected_tx: Sender<StdbDisconnectedMessage>,
<<<<<<< HEAD
=======
    /// Sender used by the SpacetimeDB on-connection error callback.
    connect_error_tx: Sender<StdbConnectErrorMessage>,
>>>>>>> origin/main
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
            client_id: self.client_id.clone(),
            id_token: self.id_token.clone(),
            driver: self.driver.clone(),
            compression: self.compression,
            connected_tx: self.connected_tx.clone(),
            disconnected_tx: self.disconnected_tx.clone(),
<<<<<<< HEAD
=======
            connect_error_tx: self.connect_error_tx.clone(),
>>>>>>> origin/main
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
<<<<<<< HEAD
        let connect_error_tx = self.disconnected_tx.clone();
=======
        let connect_error_tx = self.connect_error_tx.clone();
>>>>>>> origin/main

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
<<<<<<< HEAD
                let _ = connect_error_tx.send(StdbDisconnectedMessage { err: Some(err) });
            })
    }

    /// Updates the access token used for future connection attempts.
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub(crate) fn update_token(&mut self, token: String) {
        self.token = Some(token);
    }

    /// Updates the client ID used for token refresh.
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub(crate) fn update_client_id(&mut self, client_id: Option<String>) {
        self.client_id = client_id;
    }

    /// Clears stored authentication data from the connection config.
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub(crate) fn clear_auth(&mut self) {
        self.token = None;
        self.client_id = None;
        self.id_token = None;
    }

    /// Returns the client ID used for auth refresh, if available.
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub(crate) fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    /// Updates the ID token returned by the OIDC provider.
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub(crate) fn update_id_token(&mut self, id_token: Option<String>) {
        self.id_token = id_token;
    }

    /// Returns the ID token returned by the OIDC provider, if available.
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub(crate) fn id_token(&self) -> Option<&str> {
        self.id_token.as_deref()
    }

=======
                // TODO: waiting for STDB release with fix for this to function properly.
                let _ = connect_error_tx.send(StdbConnectErrorMessage { err });
            })
    }

>>>>>>> origin/main
    /// Builds a SpacetimeDB connection from this config.
    ///
    /// The returned connection is not started automatically.
    pub(crate) async fn build_connection(&self) -> Result<Arc<C>> {
        #[cfg(not(feature = "browser"))]
        return self.connection_builder().build().map(Arc::new);
        #[cfg(feature = "browser")]
        return self.connection_builder().build().await.map(Arc::new);
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
        register_channel::<StdbConnectedMessage>(app);
        register_channel::<StdbDisconnectedMessage>(app);
<<<<<<< HEAD
=======
        register_channel::<StdbConnectErrorMessage>(app);
>>>>>>> origin/main

        let world = app.world();
        app.insert_resource(StdbConnectionConfig::<C, M> {
            module_name: self.module_name.clone(),
            uri: self.uri.clone(),
            token: None,
            client_id: None,
            id_token: None,
            driver: self.driver.clone(),
            compression: self.compression,
            connected_tx: channel_sender::<StdbConnectedMessage>(world),
            disconnected_tx: channel_sender::<StdbDisconnectedMessage>(world),
<<<<<<< HEAD
=======
            connect_error_tx: channel_sender::<StdbConnectErrorMessage>(world),
>>>>>>> origin/main
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

        if matches!(self.driver, Some(ConnectionDriver::FrameTick(_))) {
            app.add_systems(
                PreUpdate,
                (|conn: Res<StdbConnection<C>>, config: Res<StdbConnectionConfig<C, M>>| {
                    if let Some(ConnectionDriver::FrameTick(frame_tick)) = config.driver {
                        let _ = frame_tick(conn.conn.as_ref());
                    }
                })
                .in_set(StdbSet::Connection)
                .run_if(resource_exists::<StdbConnection<C>>),
            );
        }
    }
}

/// Polls a pending connection resource per tick, advancing the connection phase when needed.
fn poll_pending_connection<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    let Some(pending_connection) = world.remove_resource::<PendingConnection<C>>() else {
        return;
    };

    match pending_connection {
        PendingConnection(mut task) => {
            let Some(result) = block_on(poll_once(&mut task)) else {
                world.insert_resource(PendingConnection::<C>(task));
                return;
            };

            match result {
                Ok(conn) => {
                    let driver = world
                        .get_resource::<StdbConnectionConfig<C, M>>()
                        .expect("StdbConnectionConfig should exist when activating a connection")
                        .driver
                        .clone();

                    if let Some(ConnectionDriver::Background(background_driver)) = driver {
                        background_driver(conn.as_ref());
                    }

                    if let Some(prev_conn) = world.get_resource::<StdbConnection<C>>() {
                        let _ = prev_conn.disconnect();
                    }
                    world.insert_resource(StdbConnection::new(conn));
                }
                Err(err) => {
<<<<<<< HEAD
                    error!("failed to build SpacetimeDB connection: {err}");
=======
                    world.write_message(StdbConnectErrorMessage { err });
                    // TODO log or send message for the error
                    // error!("failed to build SpacetimeDB connection: {err}");
>>>>>>> origin/main
                }
            }
        }
    }
}

// Ensures the StdbConnection resource is valid when it exists, otherwise it should be removed
fn sync_connection_resource<C: DbContext + Send + Sync + 'static>(
    mut connected_msgs: ReadStdbConnectedMessage,
    mut disconnected_msgs: ReadStdbDisconnectedMessage,
    conn: Option<Res<StdbConnection<C>>>,
    mut commands: Commands,
) {
    if (connected_msgs.read().next().is_some() || disconnected_msgs.read().next().is_some())
<<<<<<< HEAD
        && conn.as_ref().is_some_and(|conn| !conn.is_active()) {
            commands.remove_resource::<StdbConnection<C>>();
        }
=======
        && conn.as_ref().is_some_and(|conn| !conn.is_active())
    {
        commands.remove_resource::<StdbConnection<C>>();
    }
>>>>>>> origin/main
}

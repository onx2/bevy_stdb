//! Connection state and lifecycle for SpacetimeDB.
//!
//! Manages the active connection, lifecycle states, and related resources.
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
use crate::auth::StdbAuthRefresh;
use crate::{
    alias::{
        ReadStdbConnectedMessage, ReadStdbConnectionErrorMessage, ReadStdbDisconnectedMessage,
    },
    auth::StdbTokenResponse,
    channel_bridge::{channel_sender, register_channel},
    message::{
        RequestStdbConnectionMessage, StdbConnectedMessage, StdbConnectionErrorMessage,
        StdbDisconnectedMessage,
    },
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{
    Commands, IntoScheduleConfigs, Messages, Res, Resource, World, resource_exists,
};
use bevy_tasks::{IoTaskPool, Task, block_on, poll_once};
use crossbeam_channel::Sender;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, ConnectionId, DbConnectionBuilder, DbContext, Identity, Result,
};
use std::sync::Arc;

/// Stores the finalized connection inputs for a pending attempt.
struct PreparedConnection {
    /// The URI override for this attempt.
    uri: Option<String>,
    /// The module name override for this attempt.
    module_name: Option<String>,
    /// The token response for this attempt, if one was acquired.
    token_response: Option<StdbTokenResponse>,
    /// The client ID for this attempt, if needed.
    client_id: Option<String>,
}

/// Represents a failure while preparing a connection attempt.
#[derive(Debug)]
enum PrepareConnectionError {
    /// The requested authentication source did not produce a token response.
    TokenResponseUnavailable,
}

/// Tracks the current phase of a pending connection attempt.
enum PendingConnectionPhase<C: DbContext + Send + Sync + 'static> {
    /// Prepares the finalized connection inputs for this attempt.
    Prepare(Task<std::result::Result<PreparedConnection, PrepareConnectionError>>),
    /// Builds the SpacetimeDB connection from a finalized config snapshot.
    Build(Task<Result<Arc<C>>>),
}

/// Stores the in-flight task state for a pending connection attempt.
#[derive(Resource)]
struct PendingConnection<C: DbContext + Send + Sync + 'static> {
    /// The current phase for this pending attempt.
    phase: PendingConnectionPhase<C>,
}
impl<C: DbContext + Send + Sync + 'static> PendingConnection<C> {
    pub fn new(phase: PendingConnectionPhase<C>) -> Self {
        Self { phase }
    }
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
    /// Optional client ID for authentication.
    client_id: Option<String>,
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
            client_id: self.client_id.clone(),
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

    /// Updates the access token used for future connection attempts.
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub(crate) fn update_token(&mut self, token: String) {
        self.token = Some(token);
    }

    /// Returns the client ID used for auth refresh, if available.
    #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
    pub(crate) fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    /// Builds a SpacetimeDB connection from this config.
    ///
    /// The returned connection is not started automatically.
    pub(crate) async fn build_connection(&self) -> Result<Arc<C>> {
        #[cfg(not(target_arch = "wasm32"))]
        return self.connection_builder().build().map(Arc::new);
        #[cfg(target_arch = "wasm32")]
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
        app.add_message::<RequestStdbConnectionMessage>();

        register_channel::<StdbConnectedMessage>(app);
        register_channel::<StdbDisconnectedMessage>(app);
        register_channel::<StdbConnectionErrorMessage>(app);

        let world = app.world();
        app.insert_resource(StdbConnectionConfig::<C, M> {
            module_name: self.module_name.clone(),
            uri: self.uri.clone(),
            token: None,
            client_id: None,
            driver: self.driver.clone(),
            compression: self.compression,
            connected_tx: channel_sender::<StdbConnectedMessage>(world),
            disconnected_tx: channel_sender::<StdbDisconnectedMessage>(world),
            error_tx: channel_sender::<StdbConnectionErrorMessage>(world),
        });

        app.add_systems(
            PreUpdate,
            sync_connection_resource::<C>.in_set(StdbSet::StateSync),
        );

        app.add_systems(
            PreUpdate,
            (
                handle_connection_request::<C, M>,
                poll_pending_connection::<C, M>.run_if(resource_exists::<PendingConnection<C>>),
            )
                .chain()
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

/// Initiates a pending connection attempt from a connection request.
///
/// Requests can override the current connection configuration and while an
/// active connection exists, this will clear any pending requests.
fn handle_connection_request<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    if world.get_resource::<PendingConnection<C>>().is_some() {
        world
            .resource_mut::<Messages<RequestStdbConnectionMessage>>()
            .clear();
        return;
    }

    let Some(request) = world
        .resource_mut::<Messages<RequestStdbConnectionMessage>>()
        .drain()
        .last()
    else {
        return;
    };

    world.insert_resource(PendingConnection::<C>::new(
        PendingConnectionPhase::Prepare(IoTaskPool::get().spawn(async move {
            let (token_response, client_id) = match request.auth_source {
                Some(auth_source) => {
                    let Some(token_response) = auth_source.acquire_token_response().await else {
                        return Err(PrepareConnectionError::TokenResponseUnavailable);
                    };
                    (Some(token_response), auth_source.client_id())
                }
                None => (None, None),
            };

            Ok(PreparedConnection {
                uri: request.uri,
                module_name: request.module_name,
                token_response,
                client_id,
            })
        })),
    ));
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

    match pending_connection.phase {
        PendingConnectionPhase::Prepare(mut task) => {
            let Some(result) = block_on(poll_once(&mut task)) else {
                world.insert_resource(PendingConnection::<C> {
                    phase: PendingConnectionPhase::Prepare(task),
                });
                return;
            };

            let Ok(prepared_conn) = result else {
                return;
            };

            let connect_config = {
                let mut config = world.resource_mut::<StdbConnectionConfig<C, M>>();
                if let Some(uri) = prepared_conn.uri {
                    config.uri = uri;
                }
                if let Some(module_name) = prepared_conn.module_name {
                    config.module_name = module_name;
                }
                if let Some(token_response) = prepared_conn.token_response.as_ref() {
                    config.token = Some(token_response.access_token.clone());
                }
                if let Some(client_id) = prepared_conn.client_id {
                    config.client_id = Some(client_id);
                }
                config.clone()
            };

            // Auto-refresh behavior only available when using OIDC or Steam auth
            #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
            {
                if let Some(auth_refresh) = prepared_conn
                    .token_response
                    .as_ref()
                    .and_then(StdbAuthRefresh::from_token_response)
                {
                    world.insert_resource(auth_refresh);
                } else {
                    world.remove_resource::<StdbAuthRefresh>();
                }
            }

            world.insert_resource(PendingConnection::<C>::new(PendingConnectionPhase::Build(
                IoTaskPool::get().spawn(async move { connect_config.build_connection().await }),
            )));
        }
        PendingConnectionPhase::Build(mut task) => {
            let Some(result) = block_on(poll_once(&mut task)) else {
                world.insert_resource(PendingConnection::<C> {
                    phase: PendingConnectionPhase::Build(task),
                });
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
                Err(_) => { /* TBD */ }
            }
        }
    }
}

// Ensures the StdbConnection resource is valid when it exists, otherwise it should be removed
fn sync_connection_resource<C: DbContext + Send + Sync + 'static>(
    mut connected_msgs: ReadStdbConnectedMessage,
    mut disconnected_msgs: ReadStdbDisconnectedMessage,
    mut connection_error_msgs: ReadStdbConnectionErrorMessage,
    conn: Option<Res<StdbConnection<C>>>,
    mut commands: Commands,
) {
    if connected_msgs.read().next().is_some()
        || disconnected_msgs.read().next().is_some()
        || connection_error_msgs.read().next().is_some()
    {
        if conn.as_ref().is_some_and(|conn| !conn.is_active()) {
            commands.remove_resource::<StdbConnection<C>>();
        }
    }
}

//! Bevy message types for SpacetimeDB connection, subscription, and table events.

use bevy_ecs::prelude::Message;
use spacetimedb_sdk::{
    __codegen::{AbstractEventContext, InModule, SpacetimeModule},
    Error, Identity,
};

/// Event metadata associated with row callbacks for a SpacetimeDB row type.
pub type RowEvent<T> =
    <<<T as InModule>::Module as SpacetimeModule>::EventContext as AbstractEventContext>::Event;

/// A [`Message`] sent when a SpacetimeDB connection is established.
#[derive(Message, Debug)]
pub struct StdbConnectedMessage {
    /// The connection [`Identity`].
    pub identity: Identity,
    /// A private access token for reconnecting as the same [`Identity`].
    pub access_token: String,
}

/// Why a SpacetimeDB connection closed without an SDK error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectIntent {
    /// This client closed the connection, through
    /// [`StdbConnection::disconnect`](crate::prelude::StdbConnection::disconnect) or
    /// [`StdbCommands`](crate::prelude::StdbCommands).
    Requested,
    /// The connection closed without being requested, as it does when the server goes away.
    ///
    /// Reconnect retries a lost connection the same way it retries an error. A close made through
    /// an SDK context, such as `ctx.disconnect()` in a reducer callback, is also reported as lost;
    /// disconnect through [`StdbConnection::disconnect`](crate::prelude::StdbConnection::disconnect)
    /// or [`StdbCommands::disconnect`](crate::prelude::StdbCommands::disconnect) instead.
    Lost,
}

/// A [`Message`] sent when a SpacetimeDB connection is closed or lost.
#[derive(Message, Debug)]
pub struct StdbDisconnectedMessage {
    /// Why the connection closed, or the error the SDK reported for it.
    pub result: Result<DisconnectIntent, Error>,
}

impl StdbDisconnectedMessage {
    /// Returns `true` if this client requested the disconnect.
    pub fn was_requested(&self) -> bool {
        matches!(self.result, Ok(DisconnectIntent::Requested))
    }
}

/// A [`Message`] sent when a SpacetimeDB connection fails to connect.
#[derive(Message, Debug)]
pub struct StdbConnectErrorMessage {
    /// The error that caused the connection attempt to fail.
    pub err: Error,
}

/// A [`Message`] sent when reconnecting gives up.
///
/// Sent once [`StdbReconnectOptions::max_attempts`](crate::prelude::StdbReconnectOptions::max_attempts)
/// attempts have failed. No further attempt is made until one is requested with
/// [`StdbCommands::connect`](crate::prelude::StdbCommands::connect) or
/// [`StdbCommands::reconnect`](crate::prelude::StdbCommands::reconnect).
#[derive(Message, Clone, Debug)]
pub struct StdbReconnectExhaustedMessage {
    /// The number of attempts made.
    pub attempts: u32,
}

/// A [`Message`] sent when a frame-driven connection fails to advance.
///
/// Only a connection configured with
/// [`StdbPlugin::with_frame_driver`](crate::prelude::StdbPlugin::with_frame_driver) sends this.
/// Errors that end the connection are reported through
/// [`ReadStdbDisconnectedMessage`](crate::prelude::ReadStdbDisconnectedMessage), or through
/// [`ReadStdbConnectErrorMessage`](crate::prelude::ReadStdbConnectErrorMessage) if the connection
/// never finished connecting.
#[derive(Message, Debug)]
pub struct StdbDriverErrorMessage {
    /// The error the driver returned.
    pub err: Error,
}

/// A [`Message`] sent when a subscription is applied.
#[derive(Message, Clone, Debug)]
pub struct StdbSubscriptionAppliedMessage<K> {
    /// The subscription key associated with the applied subscription.
    pub key: K,
}
impl<K: PartialEq> StdbSubscriptionAppliedMessage<K> {
    /// Returns `true` when this message belongs to `key`.
    pub fn is(&self, key: &K) -> bool {
        &self.key == key
    }
}

/// A [`Message`] sent when a subscription application fails.
#[derive(Message, Clone, Debug)]
pub struct StdbSubscriptionErrorMessage<K> {
    /// The subscription key associated with the failed subscription.
    pub key: K,
    /// The subscription error.
    pub err: Error,
}
impl<K: PartialEq> StdbSubscriptionErrorMessage<K> {
    /// Returns `true` when this message belongs to `key`.
    pub fn is(&self, key: &K) -> bool {
        &self.key == key
    }
}

/// A [`Message`] sent when a row is inserted into a subscribed table.
#[derive(Message, Debug)]
pub struct InsertMessage<T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    /// The SpacetimeDB event that triggered the row callback.
    pub event: RowEvent<T>,
    /// The affected row.
    pub row: T,
}

/// A [`Message`] sent when a row is deleted from a subscribed table.
#[derive(Message, Debug)]
pub struct DeleteMessage<T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    /// The SpacetimeDB event that triggered the row callback.
    pub event: RowEvent<T>,
    /// The affected row.
    pub row: T,
}

/// A [`Message`] sent when a row in a subscribed table is updated.
#[derive(Message, Debug)]
pub struct UpdateMessage<T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    /// The SpacetimeDB event that triggered the row callback.
    pub event: RowEvent<T>,
    /// The previous row value.
    pub old: T,
    /// The updated row value.
    pub new: T,
}

/// A [`Message`] sent when a row in a subscribed table is inserted or updated.
#[derive(Message, Debug)]
pub struct InsertUpdateMessage<T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    /// The SpacetimeDB event that triggered the row callback.
    pub event: RowEvent<T>,
    /// The previous row value, if this was an update.
    pub old: Option<T>,
    /// The current row value.
    pub new: T,
}

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

/// A [`Message`] sent when a SpacetimeDB connection is closed or lost.
#[derive(Message, Debug)]
pub struct StdbDisconnectedMessage {
    /// The error that caused the disconnect, if any.
    pub err: Option<Error>,
}

/// A [`Message`] sent when a SpacetimeDB connection fails to connect.
#[derive(Message, Debug)]
pub struct StdbConnectErrorMessage {
    /// The error that caused the connection attempt to fail.
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

/// A [`Message`] sent when a subscribed table row changes.
///
/// The insert, delete, and update variants share one channel and are forwarded from the same
/// SDK callbacks as the corresponding typed messages. For one row type and connection driver,
/// values retain SDK callback order without cross-type channel reordering.
///
/// The SDK may group or coalesce rows while applying a transaction diff, so this is not an
/// operation log and does not expose the order of mutations within one server transaction.
/// Streams for different row types have no ordering relationship.
#[derive(Message, Debug)]
pub enum TableChange<T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    /// The row was inserted.
    Insert {
        /// The SpacetimeDB event that triggered the row callback.
        event: RowEvent<T>,
        /// The inserted row.
        row: T,
    },
    /// The row was deleted.
    Delete {
        /// The SpacetimeDB event that triggered the row callback.
        event: RowEvent<T>,
        /// The deleted row.
        row: T,
    },
    /// The row was updated.
    Update {
        /// The SpacetimeDB event that triggered the row callback.
        event: RowEvent<T>,
        /// The previous row value.
        old: T,
        /// The updated row value.
        new: T,
    },
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

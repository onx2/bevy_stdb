//! Bevy message types for SpacetimeDB connection, subscription, and table events.

use bevy_ecs::prelude::Message;
use spacetimedb_sdk::{
    __codegen::{AbstractEventContext, InModule, SpacetimeModule},
    Error, Identity,
};

/// Framework-owned envelope that bridges a value into Bevy from any callback or
/// thread running off the Bevy schedule — a reducer or procedure `_then`
/// handler, an HTTP response handler, a background task, and so on.
///
/// `T` is whatever you want to deliver, including the call result and any
/// correlation data. Owning the envelope here keeps your payload a plain
/// `struct`/`enum` with no Bevy derives.
///
/// Register with
/// [`StdbPlugin::add_custom_message`](crate::prelude::StdbPlugin::add_custom_message),
/// forward with `tx.send(StdbCustomMessage(value))` from a sender obtained via
/// [`StdbChannels::sender`](crate::prelude::StdbChannels::sender), and read with
/// [`ReadStdbCustomMessage<T>`](crate::prelude::ReadStdbCustomMessage).
#[derive(Message)]
pub struct StdbCustomMessage<T: Send + Sync + 'static>(pub T);

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

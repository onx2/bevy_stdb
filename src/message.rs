//! Bevy message types for SpacetimeDB connection, subscription, and table events.
use crate::auth::StdbAuthTarget;
use bevy_ecs::prelude::Message;
use spacetimedb_sdk::{
    __codegen::{AbstractEventContext, InModule, SpacetimeModule},
    DbContext, Error, Identity, Result,
};
use std::sync::Arc;

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

/// A [`Message`] sent when a SpacetimeDB connection attempt fails.
#[derive(Message, Debug)]
pub struct StdbConnectionErrorMessage {
    /// The connection error.
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

/// Requests a SpacetimeDB connection attempt.
///
/// If any field is `Some`, it overrides the currently stored value and becomes the
/// value used for this attempt and future reconnect attempts.
#[derive(Message, Clone, Debug, Default)]
pub struct RequestStdbConnectionMessage {
    /// Optional authentication options for the connection
    pub auth_target: Option<StdbAuthTarget>,
    /// Optional URI to use for this connection attempt.
    pub uri: Option<String>,
    /// Optional module name to use for this connection attempt.
    pub module_name: Option<String>,
}

/// Internal completion message for a finished connection build.
#[derive(Message)]
pub(crate) struct ConnectionBuildFinishedMessage<C: DbContext + Send + Sync + 'static> {
    pub result: Result<Arc<C>>,
}

/// An internal message requesting the "/token" endpoint to respond with a token given.
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
#[derive(Message, Clone, Debug)]
pub(crate) enum RequestStdbTokenMessage {
    Oidc((String /* code */, bool /* is_refresh? */)),
    Steam(String /* steam_ticket */),
}

// IDK if this should be separate... seems weird to request auth but not connect
// /// A message requesting the SpacetimeAuth flow to start
// #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
// #[derive(Message, Clone, Debug)]
// pub struct RequestStdbAuthMessage(pub StdbAuthTarget);

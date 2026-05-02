//! Bevy message types for SpacetimeDB connection, subscription, and table events.
use crate::auth::StdbAuthSource;
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
pub struct StdbConnectRequest {
    /// Optional [`StdbAuthSource`] for the connection.
    pub auth_source: Option<StdbAuthSource>,
    /// Optional URI for this connection attempt.
    pub uri: Option<String>,
    /// Optional module name for this connection attempt.
    pub module_name: Option<String>,
}

impl StdbConnectRequest {
    /// Creates a [`StdbConnectRequest`] with an authentication source.
    pub fn with_auth(auth_source: StdbAuthSource) -> Self {
        Self {
            auth_source: Some(auth_source),
            uri: None,
            module_name: None,
        }
    }

    /// Creates a [`StdbConnectRequest`] with a URI.
    pub fn with_uri(uri: impl Into<String>) -> Self {
        Self {
            auth_source: None,
            uri: Some(uri.into()),
            module_name: None,
        }
    }

    /// Creates a [`StdbConnectRequest`] with a module name.
    pub fn with_module_name(module_name: impl Into<String>) -> Self {
        Self {
            auth_source: None,
            uri: None,
            module_name: Some(module_name.into()),
        }
    }

    /// Creates a [`StdbConnectRequest`] with a URI and module name.
    pub fn with_target(uri: impl Into<String>, module_name: impl Into<String>) -> Self {
        Self {
            auth_source: None,
            uri: Some(uri.into()),
            module_name: Some(module_name.into()),
        }
    }
}

/// Requests a SpacetimeDB disconnection.
#[derive(Message, Clone, Debug, Default)]
pub struct StdbDisconnectRequest {
    /// Clears stored authentication state when `true`.
    pub forget_auth: bool,
}

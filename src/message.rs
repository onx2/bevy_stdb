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

/// Options for authenticating with SpacetimeDB.
#[derive(Clone, Debug)]
pub struct StdbLoginOptions {
    /// The authentication source used to acquire an access token.
    pub auth_source: StdbAuthSource,
}

impl StdbLoginOptions {
    /// Creates [`StdbLoginOptions`] with the given [`StdbAuthSource`].
    pub fn new(auth_source: StdbAuthSource) -> Self {
        Self { auth_source }
    }
}

/// Options for clearing stored SpacetimeDB authentication.
#[derive(Clone, Debug)]
pub struct StdbLogoutOptions {
    /// Clears the in-memory authentication session when `true`.
    pub clear_memory_session: bool,
    /// Clears the stored refresh token when `true`.
    pub clear_stored_refresh_token: bool,
}

impl Default for StdbLogoutOptions {
    fn default() -> Self {
        Self {
            clear_memory_session: true,
            clear_stored_refresh_token: true,
        }
    }
}

/// Options for starting a SpacetimeDB connection attempt.
#[derive(Clone, Debug, Default)]
pub struct StdbConnectOptions {
    /// Optional access token for this connection attempt.
    pub token: Option<String>,
    /// Optional URI for this connection attempt.
    pub uri: Option<String>,
    /// Optional module name for this connection attempt.
    pub module_name: Option<String>,
}

impl StdbConnectOptions {
    /// Creates [`StdbConnectOptions`] with an access token.
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            uri: None,
            module_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI.
    pub fn with_uri(uri: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            module_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a module name.
    pub fn with_module_name(module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: None,
            module_name: Some(module_name.into()),
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI and module name.
    pub fn with_target(uri: impl Into<String>, module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            module_name: Some(module_name.into()),
        }
    }
}

/// Options for disconnecting from SpacetimeDB.
#[derive(Clone, Debug, Default)]
pub struct StdbDisconnectOptions;

/// Requests SpacetimeDB authentication.
#[derive(Message, Clone, Debug)]
pub(crate) struct StdbLoginRequest {
    /// The login options.
    pub options: StdbLoginOptions,
}

/// Requests stored SpacetimeDB authentication to be cleared.
#[derive(Message, Clone, Debug, Default)]
pub(crate) struct StdbLogoutRequest {
    /// The logout options.
    pub options: StdbLogoutOptions,
}

/// Requests a SpacetimeDB connection attempt.
#[derive(Message, Clone, Debug, Default)]
pub(crate) struct StdbConnectRequest {
    /// The connection options.
    pub options: StdbConnectOptions,
}

/// Requests a SpacetimeDB disconnection.
#[derive(Message, Clone, Debug, Default)]
pub(crate) struct StdbDisconnectRequest {
    /// The disconnection options.
    pub options: StdbDisconnectOptions,
}

/// A [`Message`] sent when SpacetimeDB authentication succeeds.
#[derive(Message, Clone, Debug)]
pub struct StdbLoginSucceededMessage;

/// A [`Message`] sent when SpacetimeDB authentication fails.
#[derive(Message, Clone, Debug)]
pub struct StdbLoginFailedMessage {
    /// The failure message.
    pub message: String,
}

//! Bevy message types for SpacetimeDB connection lifecycle and table events.
use bevy_ecs::prelude::Message;
use spacetimedb_sdk::{Error, Identity};

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

/// A [`Message`] sent when a row is inserted into a subscribed table.
#[derive(Message, Debug)]
pub struct InsertMessage<T> {
    /// The affected row.
    pub row: T,
}

/// A [`Message`] sent when a row is deleted from a subscribed table.
#[derive(Message, Debug)]
pub struct DeleteMessage<T> {
    /// The affected row.
    pub row: T,
}

/// A [`Message`] sent when a row in a subscribed table is updated.
#[derive(Message, Debug)]
pub struct UpdateMessage<T> {
    /// The previous row value.
    pub old: T,
    /// The updated row value.
    pub new: T,
}

/// A [`Message`] sent when a row in a subscribed table is inserted or updated.
#[derive(Message, Debug)]
pub struct InsertUpdateMessage<T> {
    /// The previous row value, if this was an update.
    pub old: Option<T>,
    /// The current row value.
    pub new: T,
}

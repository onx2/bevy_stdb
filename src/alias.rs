//! Read-only [`MessageReader`] aliases for connection lifecycle and table events.
use crate::message::{
    DeleteMessage, InsertMessage, InsertUpdateMessage, StdbConnectErrorMessage,
    StdbConnectedMessage, StdbDisconnectedMessage, StdbDriverErrorMessage,
    StdbReconnectExhaustedMessage, StdbSubscriptionAppliedMessage, StdbSubscriptionErrorMessage,
    UpdateMessage,
};
use bevy_ecs::prelude::MessageReader;

/// Reads insert events for rows of `T`.
pub type ReadInsertMessage<'w, 's, T> = MessageReader<'w, 's, InsertMessage<T>>;

/// Reads update events for rows of `T`.
pub type ReadUpdateMessage<'w, 's, T> = MessageReader<'w, 's, UpdateMessage<T>>;

/// Reads delete events for rows of `T`.
pub type ReadDeleteMessage<'w, 's, T> = MessageReader<'w, 's, DeleteMessage<T>>;

/// Reads insert-or-update events for rows of `T`.
pub type ReadInsertUpdateMessage<'w, 's, T> = MessageReader<'w, 's, InsertUpdateMessage<T>>;

/// Reads successful SpacetimeDB connections.
pub type ReadStdbConnectedMessage<'w, 's> = MessageReader<'w, 's, StdbConnectedMessage>;

/// Reads closed or lost SpacetimeDB connections.
pub type ReadStdbDisconnectedMessage<'w, 's> = MessageReader<'w, 's, StdbDisconnectedMessage>;

/// Reads failed SpacetimeDB connection attempts.
pub type ReadStdbConnectErrorMessage<'w, 's> = MessageReader<'w, 's, StdbConnectErrorMessage>;

/// Reads the reconnect cycle giving up, once
/// [`StdbReconnectOptions::max_attempts`](crate::prelude::StdbReconnectOptions::max_attempts)
/// attempts have failed.
pub type ReadStdbReconnectExhaustedMessage<'w, 's> =
    MessageReader<'w, 's, StdbReconnectExhaustedMessage>;

/// Reads errors from advancing a connection configured with
/// [`StdbPlugin::with_frame_driver`](crate::prelude::StdbPlugin::with_frame_driver).
pub type ReadStdbDriverErrorMessage<'w, 's> = MessageReader<'w, 's, StdbDriverErrorMessage>;

/// Reads successful subscription applications keyed by `K`.
pub type ReadStdbSubscriptionAppliedMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionAppliedMessage<K>>;

/// Reads failed subscription applications keyed by `K`.
pub type ReadStdbSubscriptionErrorMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionErrorMessage<K>>;

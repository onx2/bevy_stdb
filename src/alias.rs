//! Read-only [`MessageReader`] aliases for connection lifecycle and table events.
use crate::message::{
    DeleteMessage, InsertMessage, InsertUpdateMessage, StdbConnectErrorMessage,
    StdbConnectedMessage, StdbDisconnectedMessage, StdbSubscriptionAppliedMessage,
    StdbSubscriptionErrorMessage, TableChange, UpdateMessage,
};
use bevy_ecs::prelude::MessageReader;

/// Reads ordered table changes for rows of `T`.
///
/// Register the table with [`StdbPlugin::add_table`](crate::prelude::StdbPlugin::add_table)
/// or an insert, delete, or update capability, then read all change kinds from one stream.
///
/// ```ignore
/// fn read_changes(mut changes: ReadTableChangeMessage<'_, '_, PlayerRow>) {
///     for change in changes.read() {
///         match change {
///             TableChange::Insert { row, .. } => { /* use `row` */ }
///             TableChange::Update { old, new, .. } => { /* use `old` and `new` */ }
///             TableChange::Delete { row, .. } => { /* use `row` */ }
///         }
///     }
/// }
/// ```
pub type ReadTableChangeMessage<'w, 's, T> = MessageReader<'w, 's, TableChange<T>>;

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

/// Reads successful subscription applications keyed by `K`.
pub type ReadStdbSubscriptionAppliedMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionAppliedMessage<K>>;

/// Reads failed subscription applications keyed by `K`.
pub type ReadStdbSubscriptionErrorMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionErrorMessage<K>>;

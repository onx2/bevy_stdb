//! Bevy message types for SpacetimeDB connection, subscription, and table events.

use bevy_ecs::prelude::Message;
use spacetimedb_sdk::{
    __codegen::{AbstractEventContext, InModule, SpacetimeModule},
    Error, Identity,
};
use std::{any::type_name, fmt};

/// Event metadata associated with row callbacks for a SpacetimeDB row type.
pub type RowEvent<T> =
    <<<T as InModule>::Module as SpacetimeModule>::EventContext as AbstractEventContext>::Event;

/// The SpacetimeDB event behind one table change, carried only for row types that opted in with
/// [`StdbPlugin::with_event`](crate::prelude::StdbPlugin::with_event).
///
/// Read it with [`TableChange::event`] or `event()` on what a table reader yields, or with
/// `try_event()` where a row type may not carry it. Build one with `Some(event).into()` or
/// `None.into()`, as a test that sends its own `TableChange` does. It is boxed, so a change without
/// one costs a null pointer rather than the size of the event.
pub struct MaybeRowEvent<T: InModule>(Option<Box<RowEvent<T>>>);

impl<T: InModule> MaybeRowEvent<T> {
    /// Returns the event, or panics naming the opt-in when there is none.
    #[track_caller]
    pub(crate) fn get(&self) -> &RowEvent<T> {
        self.try_get().unwrap_or_else(|| {
            panic!(
                "`{}` changes carry no SpacetimeDB event; opt in with `StdbPlugin::with_event` \
                 for its table accessor, or read it with `try_event`",
                type_name::<T>(),
            )
        })
    }

    pub(crate) fn try_get(&self) -> Option<&RowEvent<T>> {
        self.0.as_deref()
    }
}

impl<T: InModule> From<Option<RowEvent<T>>> for MaybeRowEvent<T> {
    fn from(event: Option<RowEvent<T>>) -> Self {
        Self(event.map(Box::new))
    }
}

impl<T: InModule> Clone for MaybeRowEvent<T>
where
    RowEvent<T>: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: InModule> fmt::Debug for MaybeRowEvent<T>
where
    RowEvent<T>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

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
    /// This client asked for the close, through
    /// [`StdbConnection::disconnect`](crate::prelude::StdbConnection::disconnect) or
    /// [`StdbCommands`](crate::prelude::StdbCommands).
    Requested,
    /// The connection closed on its own and the SDK reported no error -- which is what the SDK
    /// reports when the server goes away, so this is retried like an error.
    Lost,
}

/// A [`Message`] sent when a SpacetimeDB connection is closed or lost.
#[derive(Message, Debug)]
pub struct StdbDisconnectedMessage {
    /// Why the connection closed, or the error the SDK reported for it.
    pub result: Result<DisconnectIntent, Error>,
}

impl StdbDisconnectedMessage {
    /// Returns `true` when this client asked for the disconnect, so nothing should retry it.
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

/// A [`Message`] sent when the reconnect cycle gives up.
///
/// Sent once [`StdbReconnectOptions::max_attempts`](crate::prelude::StdbReconnectOptions::max_attempts)
/// attempts have failed. Nothing retries after this until a connection succeeds or is requested
/// again with [`StdbCommands::connect`](crate::prelude::StdbCommands::connect).
#[derive(Message, Clone, Debug)]
pub struct StdbReconnectExhaustedMessage {
    /// The number of attempts that were made.
    pub attempts: u32,
}

/// A [`Message`] sent when a frame-driven connection fails to advance.
///
/// Only a connection configured with
/// [`StdbPlugin::with_frame_driver`](crate::prelude::StdbPlugin::with_frame_driver) sends this. An
/// error that ends the connection is not reported here; it arrives through
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

/// A [`Message`] sent when a subscribed table row changes.
///
/// This is the one stream the SDK row callbacks feed, and the only place a row is stored.
/// [`ReadInsertMessage`](crate::prelude::ReadInsertMessage) and its siblings are views over it
/// that filter to one change kind and borrow the row out. For one row type and connection
/// driver, values retain SDK callback order without cross-type channel reordering.
///
/// The SDK may group or coalesce rows while applying a transaction diff, so this is not an
/// operation log and does not expose the order of mutations within one server transaction.
/// Streams for different row types have no ordering relationship.
///
/// The stream is keyed by row type, not by accessor: binding both a table and a view over one
/// row type merges their callbacks here, and a change seen by both arrives twice with nothing
/// to tell them apart. Subscribe to one accessor per row type when that matters.
#[derive(Message, Debug)]
pub enum TableChange<T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    /// The row was inserted.
    Insert {
        /// The SpacetimeDB event behind the change; read it with [`TableChange::event`].
        event: MaybeRowEvent<T>,
        /// The inserted row.
        row: T,
    },
    /// The row was deleted.
    Delete {
        /// The SpacetimeDB event behind the change; read it with [`TableChange::event`].
        event: MaybeRowEvent<T>,
        /// The deleted row.
        row: T,
    },
    /// The row was updated.
    Update {
        /// The SpacetimeDB event behind the change; read it with [`TableChange::event`].
        event: MaybeRowEvent<T>,
        /// The previous row value.
        old: T,
        /// The updated row value.
        new: T,
    },
}

impl<T> TableChange<T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    /// The SpacetimeDB event that caused this change.
    ///
    /// # Panics
    ///
    /// Panics if the row type did not opt in with
    /// [`StdbPlugin::with_event`](crate::prelude::StdbPlugin::with_event).
    #[track_caller]
    pub fn event(&self) -> &RowEvent<T> {
        self.maybe_event().get()
    }

    /// The SpacetimeDB event that caused this change, or `None` if the row type did not opt in
    /// with [`StdbPlugin::with_event`](crate::prelude::StdbPlugin::with_event).
    pub fn try_event(&self) -> Option<&RowEvent<T>> {
        self.maybe_event().try_get()
    }

    fn maybe_event(&self) -> &MaybeRowEvent<T> {
        match self {
            Self::Insert { event, .. }
            | Self::Delete { event, .. }
            | Self::Update { event, .. } => event,
        }
    }
}

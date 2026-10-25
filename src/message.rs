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

/// The SpacetimeDB context behind one table change: the `ctx.event` the SDK passed to its row
/// callback.
///
/// Only row types bound with [`TableCapability::ctx`](crate::prelude::TableCapability::ctx) carry
/// it. Read it with [`TableChange::ctx`], or with `ctx()` on what a table reader yields.
pub struct RowCtx<'a, T: InModule> {
    /// The event behind the change: the reducer call that caused it, or the subscription that
    /// delivered the row.
    pub event: &'a RowEvent<T>,
}

impl<T: InModule> Clone for RowCtx<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: InModule> Copy for RowCtx<'_, T> {}

impl<T: InModule> fmt::Debug for RowCtx<'_, T>
where
    RowEvent<T>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RowCtx").field("event", self.event).finish()
    }
}

/// The SpacetimeDB context a table change carries, if its row type was bound with
/// [`TableCapability::ctx`](crate::prelude::TableCapability::ctx).
///
/// Read it with [`TableChange::ctx`] or `ctx()` on what a table reader yields, or with `try_ctx()`
/// where a row type may not carry it. Build one with `Some(event).into()` or `None.into()`, as a
/// test that sends its own `TableChange` does. The event is boxed, so a change without one costs a
/// null pointer rather than the size of the event.
pub struct MaybeRowCtx<T: InModule>(Option<Box<RowEvent<T>>>);

impl<T: InModule> MaybeRowCtx<T> {
    /// Returns the context, or panics naming the opt-in when there is none.
    #[track_caller]
    pub(crate) fn get(&self) -> RowCtx<'_, T> {
        self.try_get().unwrap_or_else(|| {
            panic!(
                "`{}` changes carry no SpacetimeDB ctx; bind `TableCapability::ctx()` for its \
                 table accessor, or read it with `try_ctx`",
                type_name::<T>(),
            )
        })
    }

    pub(crate) fn try_get(&self) -> Option<RowCtx<'_, T>> {
        self.0.as_deref().map(|event| RowCtx { event })
    }
}

impl<T: InModule> From<Option<RowEvent<T>>> for MaybeRowCtx<T> {
    fn from(event: Option<RowEvent<T>>) -> Self {
        Self(event.map(Box::new))
    }
}

impl<T: InModule> Clone for MaybeRowCtx<T>
where
    RowEvent<T>: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: InModule> fmt::Debug for MaybeRowCtx<T>
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

/// A [`Message`] sent when a subscribed table row changes.
///
/// The SDK row callbacks for a row type all write to this one stream, in the order the SDK invokes
/// them. [`ReadInsertMessage`](crate::prelude::ReadInsertMessage) and its siblings read it filtered
/// to one change kind.
///
/// The SDK may group or coalesce rows while applying a transaction diff, so this is not an
/// operation log and does not expose the order of mutations within one server transaction.
/// Streams for different row types have no ordering relationship.
///
/// The stream is keyed by row type: binding both a table and a view over one row type merges their
/// callbacks here, and a change seen by both arrives twice with nothing to tell them apart.
/// Subscribe to one accessor per row type when that matters.
#[derive(Message, Debug)]
pub enum TableChange<T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    /// The row was inserted.
    Insert {
        /// The SpacetimeDB context behind the change; read it with [`TableChange::ctx`].
        ctx: MaybeRowCtx<T>,
        /// The inserted row.
        row: T,
    },
    /// The row was deleted.
    Delete {
        /// The SpacetimeDB context behind the change; read it with [`TableChange::ctx`].
        ctx: MaybeRowCtx<T>,
        /// The deleted row.
        row: T,
    },
    /// The row was updated.
    Update {
        /// The SpacetimeDB context behind the change; read it with [`TableChange::ctx`].
        ctx: MaybeRowCtx<T>,
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
    /// The SpacetimeDB context behind this change.
    ///
    /// # Panics
    ///
    /// Panics if the row type was not bound with
    /// [`TableCapability::ctx`](crate::prelude::TableCapability::ctx).
    #[track_caller]
    pub fn ctx(&self) -> RowCtx<'_, T> {
        self.maybe_ctx().get()
    }

    /// The SpacetimeDB context behind this change, or `None` if the row type was not bound with
    /// [`TableCapability::ctx`](crate::prelude::TableCapability::ctx).
    pub fn try_ctx(&self) -> Option<RowCtx<'_, T>> {
        self.maybe_ctx().try_get()
    }

    fn maybe_ctx(&self) -> &MaybeRowCtx<T> {
        match self {
            Self::Insert { ctx, .. } | Self::Delete { ctx, .. } | Self::Update { ctx, .. } => ctx,
        }
    }
}

//! Readers for connection, subscription, and table messages.
//!
//! Connection and subscription readers are [`MessageReader`] aliases. The table readers filter a
//! row type's [`TableChange`] stream to the change kind they name, and borrow each row from it.
use crate::{
    message::{
        MaybeRowCtx, RowCtx, RowEvent, StdbConnectErrorMessage, StdbConnectedMessage,
        StdbDisconnectedMessage, StdbDriverErrorMessage, StdbReconnectExhaustedMessage,
        StdbSubscriptionAppliedMessage, StdbSubscriptionErrorMessage, TableChange,
    },
    table::{BoundStreams, RowCallback},
};
use bevy_ecs::{
    message::{MessageCursor, MessageId, Messages},
    prelude::{Local, MessageReader, Res},
    system::SystemParam,
};
use spacetimedb_sdk::__codegen::InModule;

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

/// An inserted row, borrowed from the [`TableChange`] stream.
#[derive(Debug)]
pub struct Inserted<'a, T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    ctx: &'a MaybeRowCtx<T>,
    /// The inserted row.
    pub row: &'a T,
}

/// A deleted row, borrowed from the [`TableChange`] stream.
#[derive(Debug)]
pub struct Deleted<'a, T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    ctx: &'a MaybeRowCtx<T>,
    /// The deleted row.
    pub row: &'a T,
}

/// An updated row, borrowed from the [`TableChange`] stream.
#[derive(Debug)]
pub struct Updated<'a, T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    ctx: &'a MaybeRowCtx<T>,
    /// The previous row value.
    pub old: &'a T,
    /// The updated row value.
    pub new: &'a T,
}

/// An inserted or updated row, borrowed from the [`TableChange`] stream.
#[derive(Debug)]
pub struct InsertedOrUpdated<'a, T>
where
    T: InModule,
    RowEvent<T>: Send + Sync,
{
    ctx: &'a MaybeRowCtx<T>,
    /// The previous row value, if this was an update.
    pub old: Option<&'a T>,
    /// The current row value.
    pub new: &'a T,
}

/// Implements `ctx` and `try_ctx` on a reader's item, borrowing the context from its
/// [`TableChange`].
macro_rules! ctx_accessors {
    ($($item:ident),+) => {$(
        impl<'a, T> $item<'a, T>
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
            pub fn ctx(&self) -> RowCtx<'a, T> {
                self.ctx.get()
            }

            /// The SpacetimeDB context behind this change, or `None` if the row type was not bound
            /// with [`TableCapability::ctx`](crate::prelude::TableCapability::ctx).
            pub fn try_ctx(&self) -> Option<RowCtx<'a, T>> {
                self.ctx.try_get()
            }
        }
    )+};
}

ctx_accessors!(Inserted, Deleted, Updated, InsertedOrUpdated);

/// Defines a reader that filters the [`TableChange`] stream to one change kind.
///
/// The reader has [`MessageReader`]'s methods, each limited to its change kind. It keeps its own
/// [`MessageCursor`] so that `len` and `is_empty` can look at unread changes without reading them.
macro_rules! table_reader {
    (
        $(#[$doc:meta])*
        $reader:ident -> $item:ident, [$($needs:ident),+], |$change:ident| $project:expr
    ) => {
        $(#[$doc])*
        #[derive(SystemParam)]
        pub struct $reader<'w, 's, T>
        where
            T: Send + Sync + InModule + 'static,
            RowEvent<T>: Send + Sync,
        {
            cursor: Local<'s, MessageCursor<TableChange<T>>>,
            #[system_param(validation_message = "Table changes for this row type are not registered")]
            changes: Res<'w, Messages<TableChange<T>>>,
            bound: Res<'w, BoundStreams>,
        }

        impl<T> $reader<'_, '_, T>
        where
            T: Send + Sync + InModule + 'static,
            RowEvent<T>: Send + Sync,
        {
            /// Iterates over the changes of this kind this reader has not seen yet, marking them
            /// as read.
            ///
            /// # Panics
            ///
            /// Panics if a callback this reader needs was never bound for the row type.
            #[track_caller]
            pub fn read(&mut self) -> impl Iterator<Item = $item<'_, T>> {
                self.check();
                self.cursor.read(&self.changes).filter_map(Self::project)
            }

            /// Like [`Self::read`], also returning the [`MessageId`] of each change.
            ///
            /// # Panics
            ///
            /// Panics if a callback this reader needs was never bound for the row type.
            #[track_caller]
            pub fn read_with_id(
                &mut self,
            ) -> impl Iterator<Item = ($item<'_, T>, MessageId<TableChange<T>>)> {
                self.check();
                self.cursor
                    .read_with_id(&self.changes)
                    .filter_map(|(change, id)| Self::project(change).map(|item| (item, id)))
            }

            /// Returns the number of changes of this kind this reader has not seen yet.
            ///
            /// The row type's stream holds every change kind, so this scans the unread changes
            /// rather than counting them in constant time as [`MessageReader::len`] does.
            ///
            /// # Panics
            ///
            /// Panics if a callback this reader needs was never bound for the row type.
            #[track_caller]
            pub fn len(&self) -> usize {
                self.check();
                MessageCursor::clone(&self.cursor)
                    .read(&self.changes)
                    .filter_map(Self::project)
                    .count()
            }

            /// Returns `true` if there are no changes of this kind this reader has not seen yet.
            ///
            /// # Panics
            ///
            /// Panics if a callback this reader needs was never bound for the row type.
            #[track_caller]
            pub fn is_empty(&self) -> bool {
                self.check();
                !MessageCursor::clone(&self.cursor)
                    .read(&self.changes)
                    .any(|change| Self::project(change).is_some())
            }

            /// Marks every change this reader has not seen yet as read, of any kind.
            pub fn clear(&mut self) {
                self.cursor.clear(&self.changes);
            }

            fn project($change: &TableChange<T>) -> Option<$item<'_, T>> {
                $project
            }

            #[track_caller]
            fn check(&self) {
                self.bound.assert_bound::<T>(&[$(RowCallback::$needs),+]);
            }
        }
    };
}

table_reader!(
    /// Reads inserted rows of `T`.
    ///
    /// Needs [`StdbPlugin::bind_insert`](crate::prelude::StdbPlugin::bind_insert), or an
    /// `add_*` method that implies it.
    ReadInsertMessage -> Inserted,
    [Insert],
    |change| match change {
        TableChange::Insert { ctx, row } => Some(Inserted { ctx, row }),
        _ => None,
    }
);

table_reader!(
    /// Reads deleted rows of `T`.
    ///
    /// Needs [`StdbPlugin::bind_delete`](crate::prelude::StdbPlugin::bind_delete), or an
    /// `add_*` method that implies it.
    ReadDeleteMessage -> Deleted,
    [Delete],
    |change| match change {
        TableChange::Delete { ctx, row } => Some(Deleted { ctx, row }),
        _ => None,
    }
);

table_reader!(
    /// Reads updated rows of `T`.
    ///
    /// Needs [`StdbPlugin::bind_update`](crate::prelude::StdbPlugin::bind_update), or an
    /// `add_*` method that implies it.
    ReadUpdateMessage -> Updated,
    [Update],
    |change| match change {
        TableChange::Update { ctx, old, new } => Some(Updated { ctx, old, new }),
        _ => None,
    }
);

table_reader!(
    /// Reads inserted and updated rows of `T` as one stream.
    ///
    /// Needs both inserts and updates bound: by
    /// [`StdbPlugin::bind_insert_update`](crate::prelude::StdbPlugin::bind_insert_update), by
    /// `bind_insert` and `bind_update` separately, or by `add_table`.
    ReadInsertUpdateMessage -> InsertedOrUpdated,
    [Insert, Update],
    |change| match change {
        TableChange::Insert { ctx, row } => Some(InsertedOrUpdated {
            ctx,
            old: None,
            new: row,
        }),
        TableChange::Update { ctx, old, new } => Some(InsertedOrUpdated {
            ctx,
            old: Some(old),
            new,
        }),
        TableChange::Delete { .. } => None,
    }
);

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

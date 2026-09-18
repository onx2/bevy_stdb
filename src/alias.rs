//! Readers for connection lifecycle messages and for table changes.
//!
//! Connection and subscription readers are plain [`MessageReader`] aliases. The table readers are
//! views over the one [`TableChange`] stream a row type has: they filter it to the change kind
//! they name and borrow out of it, so a row is copied once by the SDK callback and never again,
//! however many readers observe it.
use crate::{
    message::{
        MaybeRowEvent, RowEvent, StdbConnectErrorMessage, StdbConnectedMessage,
        StdbDisconnectedMessage, StdbSubscriptionAppliedMessage, StdbSubscriptionErrorMessage,
        TableChange,
    },
    table::{BoundStreams, TableCapabilityKind},
};
use bevy_ecs::{
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
    /// The SpacetimeDB event that triggered the row callback, unless the row type opted out.
    pub event: &'a MaybeRowEvent<T>,
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
    /// The SpacetimeDB event that triggered the row callback, unless the row type opted out.
    pub event: &'a MaybeRowEvent<T>,
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
    /// The SpacetimeDB event that triggered the row callback, unless the row type opted out.
    pub event: &'a MaybeRowEvent<T>,
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
    /// The SpacetimeDB event that triggered the row callback, unless the row type opted out.
    pub event: &'a MaybeRowEvent<T>,
    /// The previous row value, if this was an update.
    pub old: Option<&'a T>,
    /// The current row value.
    pub new: &'a T,
}

/// Defines a reader that filters the [`TableChange`] stream to one change kind.
///
/// The `Local` makes the registration check once per system rather than once per call: a reader
/// whose capability was never bound would otherwise just yield nothing forever.
macro_rules! table_reader {
    (
        $(#[$doc:meta])*
        $reader:ident -> $item:ident, $kind:ident, |$change:ident| $project:expr
    ) => {
        $(#[$doc])*
        #[derive(SystemParam)]
        pub struct $reader<'w, 's, T>
        where
            T: Send + Sync + InModule + 'static,
            RowEvent<T>: Send + Sync,
        {
            changes: MessageReader<'w, 's, TableChange<T>>,
            bound: Res<'w, BoundStreams>,
            checked: Local<'s, bool>,
        }

        impl<T> $reader<'_, '_, T>
        where
            T: Send + Sync + InModule + 'static,
            RowEvent<T>: Send + Sync,
        {
            /// Returns the changes of this kind that arrived since this reader last read.
            ///
            /// # Panics
            ///
            /// Panics if the row type never bound the capability this reader needs, which would
            /// otherwise read as a table that simply never changes.
            pub fn read(&mut self) -> impl Iterator<Item = $item<'_, T>> {
                if !*self.checked {
                    self.bound.assert_bound::<T>(TableCapabilityKind::$kind);
                    *self.checked = true;
                }

                self.changes.read().filter_map(|$change| $project)
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
    Insert,
    |change| match change {
        TableChange::Insert { event, row } => Some(Inserted { event, row }),
        _ => None,
    }
);

table_reader!(
    /// Reads deleted rows of `T`.
    ///
    /// Needs [`StdbPlugin::bind_delete`](crate::prelude::StdbPlugin::bind_delete), or an
    /// `add_*` method that implies it.
    ReadDeleteMessage -> Deleted,
    Delete,
    |change| match change {
        TableChange::Delete { event, row } => Some(Deleted { event, row }),
        _ => None,
    }
);

table_reader!(
    /// Reads updated rows of `T`.
    ///
    /// Needs [`StdbPlugin::bind_update`](crate::prelude::StdbPlugin::bind_update), or an
    /// `add_*` method that implies it.
    ReadUpdateMessage -> Updated,
    Update,
    |change| match change {
        TableChange::Update { event, old, new } => Some(Updated { event, old, new }),
        _ => None,
    }
);

table_reader!(
    /// Reads inserted and updated rows of `T` as one stream.
    ///
    /// Needs [`StdbPlugin::bind_insert_update`](crate::prelude::StdbPlugin::bind_insert_update),
    /// or an `add_*` method that implies it.
    ReadInsertUpdateMessage -> InsertedOrUpdated,
    InsertUpdate,
    |change| match change {
        TableChange::Insert { event, row } => Some(InsertedOrUpdated {
            event,
            old: None,
            new: row,
        }),
        TableChange::Update { event, old, new } => Some(InsertedOrUpdated {
            event,
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

/// Reads successful subscription applications keyed by `K`.
pub type ReadStdbSubscriptionAppliedMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionAppliedMessage<K>>;

/// Reads failed subscription applications keyed by `K`.
pub type ReadStdbSubscriptionErrorMessage<'w, 's, K> =
    MessageReader<'w, 's, StdbSubscriptionErrorMessage<K>>;

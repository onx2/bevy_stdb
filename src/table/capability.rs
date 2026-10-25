use super::{OnDelete, OnInsert, OnUpdate, TableBind, TableRegistry, TableSource};
use bevy_ecs::prelude::Resource;
use spacetimedb_sdk::__codegen::DbContext;
use std::{
    any::{TypeId, type_name},
    marker::PhantomData,
};

/// An SDK row callback: the unit of binding. Public only so the reader types can name it.
///
/// What a reader yields is a question of which of these feed its row type's stream, so
/// [`ReadInsertUpdateMessage`](crate::prelude::ReadInsertUpdateMessage) has no callback of its
/// own; it needs `Insert` and `Update`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowCallback {
    Insert,
    Delete,
    Update,
}

/// A typed table binding capability used with [`crate::prelude::StdbPlugin::bind`].
///
/// Construct capabilities with [`Self::insert`], [`Self::delete`],
/// [`Self::update`], and [`Self::insert_update`]. Each constructor requires
/// the corresponding capability trait on the generated table handle, so
/// unsupported bindings fail at compile time.
pub struct TableCapability<C: DbContext, T> {
    /// The SDK row callbacks this capability binds.
    callbacks: Vec<(RowCallback, TableBind<C>)>,
    _marker: PhantomData<fn() -> T>,
}

impl<C: DbContext, T> TableCapability<C, T> {
    /// Binds inserted rows of `T`.
    pub fn insert() -> Self
    where
        T: OnInsert<C>,
    {
        Self::of(vec![(RowCallback::Insert, T::bind_insert)])
    }

    /// Binds deleted rows of `T`.
    pub fn delete() -> Self
    where
        T: OnDelete<C>,
    {
        Self::of(vec![(RowCallback::Delete, T::bind_delete)])
    }

    /// Binds updated rows of `T`.
    pub fn update() -> Self
    where
        T: OnUpdate<C>,
    {
        Self::of(vec![(RowCallback::Update, T::bind_update)])
    }

    /// Binds inserted and updated rows of `T`: [`Self::insert`] and [`Self::update`] together,
    /// which is what [`ReadInsertUpdateMessage`](crate::prelude::ReadInsertUpdateMessage) reads.
    pub fn insert_update() -> Self
    where
        T: OnInsert<C> + OnUpdate<C>,
    {
        Self::of(vec![
            (RowCallback::Insert, T::bind_insert),
            (RowCallback::Update, T::bind_update),
        ])
    }

    fn of(callbacks: Vec<(RowCallback, TableBind<C>)>) -> Self {
        Self {
            callbacks,
            _marker: PhantomData,
        }
    }
}

impl<C: DbContext> TableRegistry<C> {
    /// Binds `capabilities` for `T`. Binding a callback that is already bound changes nothing.
    pub(crate) fn bind<T: TableSource<C>>(
        &mut self,
        capabilities: impl IntoIterator<Item = TableCapability<C, T>>,
    ) {
        for (callback, bind) in capabilities.into_iter().flat_map(|c| c.callbacks) {
            // The channel is keyed by row type, not by accessor: a table and a view over one row
            // share it, and `register_channel` panics on a second registration.
            if self.ledger.claim_channel(T::row_type()) {
                self.table_registrations.push(T::register_channel);
            }
            // The callback is per accessor: each feeds the shared channel from its own table,
            // and binding one twice would forward every row twice.
            if self
                .ledger
                .claim_callback(TypeId::of::<T>(), T::row_type(), callback)
            {
                self.table_bindings.push(bind);
            }
        }
    }
}

/// Tracks what a [`TableRegistry`] has bound, so each channel is registered and each SDK callback
/// bound exactly once however the registrations overlap.
#[derive(Default)]
pub(crate) struct BindLedger {
    /// Accessor/callback pairs bound on an SDK table handle.
    callbacks: Vec<(TypeId, RowCallback)>,
    /// Row/callback pairs that feed a row type's stream, which is what a reader asks about.
    streams: Vec<(TypeId, RowCallback)>,
}

impl BindLedger {
    /// Claims the [`TableChange`](crate::prelude::TableChange) channel for `row_type`, returning
    /// whether this caller is the first to need it and must therefore register it.
    ///
    /// Call before [`Self::claim_callback`], which is what records the row type.
    fn claim_channel(&self, row_type: TypeId) -> bool {
        !self.streams.iter().any(|(row, _)| *row == row_type)
    }

    /// Claims `callback` on `accessor`, returning whether this caller is the first to do so and
    /// must therefore bind it.
    fn claim_callback(
        &mut self,
        accessor: TypeId,
        row_type: TypeId,
        callback: RowCallback,
    ) -> bool {
        if !self.streams.contains(&(row_type, callback)) {
            self.streams.push((row_type, callback));
        }

        let key = (accessor, callback);
        if self.callbacks.contains(&key) {
            return false;
        }
        self.callbacks.push(key);
        true
    }

    /// Returns the readable streams as the resource the table readers check.
    pub(crate) fn bound_streams(&self) -> BoundStreams {
        BoundStreams {
            streams: self.streams.clone(),
        }
    }
}

/// The row/callback pairs the plugin bound, so a table reader can name the missing
/// registration instead of yielding nothing forever.
///
/// Public only because the reader types are; it carries no useful API of its own.
#[doc(hidden)]
#[derive(Resource, Default, Clone)]
pub struct BoundStreams {
    streams: Vec<(TypeId, RowCallback)>,
}

impl BoundStreams {
    /// # Panics
    ///
    /// Panics unless every callback in `needs` was bound for `TRow`.
    pub fn assert_bound<TRow: 'static>(&self, needs: &[RowCallback]) {
        for callback in needs {
            assert!(
                self.streams.contains(&(TypeId::of::<TRow>(), *callback)),
                "no `{callback:?}` callback is bound for row type `{}`; register it with the \
                 matching `StdbPlugin` method before reading it",
                type_name::<TRow>(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BindLedger, RowCallback};
    use std::any::TypeId;

    // Stand-ins for a generated table accessor, a view accessor over the same row, and two rows.
    struct MonsterTbl;
    struct MonsterAoi;
    struct MonsterRow;
    struct PlayerRow;

    const ALL: [RowCallback; 3] = [
        RowCallback::Insert,
        RowCallback::Delete,
        RowCallback::Update,
    ];

    fn ids() -> (TypeId, TypeId, TypeId) {
        (
            TypeId::of::<MonsterTbl>(),
            TypeId::of::<MonsterAoi>(),
            TypeId::of::<MonsterRow>(),
        )
    }

    #[test]
    fn the_first_callback_of_any_kind_claims_the_channel() {
        // `register_channel` panics on a duplicate, and a row whose channel is never registered
        // panics with "unregistered channel" when a connection binds it.
        for first in ALL {
            let mut ledger = BindLedger::default();
            let (tbl, _, row) = ids();

            assert!(ledger.claim_channel(row), "first was {first:?}");
            ledger.claim_callback(tbl, row, first);

            assert!(!ledger.claim_channel(row), "first was {first:?}");
        }
    }

    #[test]
    fn a_table_and_a_view_over_one_row_share_their_channel_but_not_their_callbacks() {
        // They are different SDK tables, so both must be bound, into one downstream channel.
        let mut ledger = BindLedger::default();
        let (tbl, aoi, row) = ids();

        assert!(ledger.claim_channel(row));
        assert!(ledger.claim_callback(tbl, row, RowCallback::Insert));

        assert!(!ledger.claim_channel(row));
        assert!(ledger.claim_callback(aoi, row, RowCallback::Insert));

        assert_eq!(ledger.streams.len(), 1);
    }

    #[test]
    fn binding_a_callback_again_is_harmless() {
        // `add_table` reaches `on_insert` through both `insert` and `insert_update`, and an app
        // may add a `bind_insert` of its own. Binding it twice would forward every row twice.
        let mut ledger = BindLedger::default();
        let (tbl, _, row) = ids();

        assert!(ledger.claim_callback(tbl, row, RowCallback::Insert));
        assert!(!ledger.claim_callback(tbl, row, RowCallback::Insert));
        assert!(ledger.claim_callback(tbl, row, RowCallback::Update));
    }

    #[test]
    fn distinct_row_types_claim_distinct_channels() {
        let mut ledger = BindLedger::default();
        let (tbl, _, row) = ids();
        ledger.claim_callback(tbl, row, RowCallback::Insert);

        assert!(ledger.claim_channel(TypeId::of::<PlayerRow>()));
    }

    #[test]
    fn a_reader_needing_two_callbacks_is_satisfied_by_binding_them_separately() {
        // There is no insert-update callback, only a reader that needs both.
        let mut ledger = BindLedger::default();
        let (tbl, _, row) = ids();
        ledger.claim_callback(tbl, row, RowCallback::Insert);
        ledger.claim_callback(tbl, row, RowCallback::Update);

        ledger
            .bound_streams()
            .assert_bound::<MonsterRow>(&[RowCallback::Insert, RowCallback::Update]);
    }

    #[test]
    #[should_panic(expected = "no `Update` callback is bound")]
    fn reading_a_stream_nobody_bound_says_which_one() {
        // Readers filter the shared stream, so an unbound callback would otherwise read as a
        // table that never changes.
        let mut ledger = BindLedger::default();
        let (tbl, _, row) = ids();
        ledger.claim_callback(tbl, row, RowCallback::Insert);

        ledger
            .bound_streams()
            .assert_bound::<MonsterRow>(&[RowCallback::Insert, RowCallback::Update]);
    }
}

use super::{TableBindCallback, TableRegistry, bind_delete, bind_insert, bind_update};
use crate::{
    channel_bridge::register_channel,
    message::{RowEvent, TableChange},
};
use bevy_ecs::prelude::Resource;
use spacetimedb_sdk::__codegen::{
    DbConnection, DbContext, InModule, SpacetimeModule, TableAccessor, TableLike, WithDelete,
    WithInsert, WithUpdate,
};
use std::{
    any::{TypeId, type_name},
    marker::PhantomData,
    sync::Arc,
};

/// An SDK row callback. Two capabilities can need the same one -- `Insert` and `InsertUpdate`
/// both need `on_insert` -- and it must be bound once per accessor, or each row would be
/// forwarded twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowCallback {
    Insert,
    Delete,
    Update,
}

/// A change kind a table reader can be bound for. Public only so the reader types can name it.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableCapabilityKind {
    Insert,
    Delete,
    Update,
    InsertUpdate,
}

/// A typed table binding capability used with [`crate::prelude::StdbPlugin::bind`].
///
/// Construct capabilities with [`Self::insert`], [`Self::delete`],
/// [`Self::update`], and [`Self::insert_update`]. Each constructor requires
/// the corresponding capability trait on the generated table handle, so
/// unsupported bindings fail at compile time.
pub struct TableCapability<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
    T,
> {
    kind: TableCapabilityKind,
    /// The row type this capability yields, which keys every channel it registers.
    row_type: TypeId,
    /// Registers the shared [`TableChange`] channel for the row type.
    change_registration: fn(&mut bevy_app::App),
    /// The SDK row callbacks this capability needs, bound once per accessor.
    callbacks: Vec<(RowCallback, Arc<TableBindCallback<C>>)>,
    _marker: PhantomData<fn() -> T>,
}

impl<C, M, T> TableCapability<C, M, T>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    /// Binds insert messages from `T`.
    pub fn insert() -> Self
    where
        T: TableAccessor<C::DbView> + Send + Sync + 'static,
        T::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<T::Row>: Send + Sync,
        for<'db> T::Handle<'db>: TableLike<
                Row = T::Row,
                EventContext = <<T::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithInsert,
    {
        Self {
            kind: TableCapabilityKind::Insert,
            row_type: TypeId::of::<T::Row>(),
            change_registration: register_channel::<TableChange<T::Row>>,
            callbacks: vec![(
                RowCallback::Insert,
                Arc::new(|world, db| {
                    bind_insert(world, &T::get(db));
                }),
            )],
            _marker: PhantomData,
        }
    }

    /// Binds delete messages from `T`.
    pub fn delete() -> Self
    where
        T: TableAccessor<C::DbView> + Send + Sync + 'static,
        T::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<T::Row>: Send + Sync,
        for<'db> T::Handle<'db>: TableLike<
                Row = T::Row,
                EventContext = <<T::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithDelete,
    {
        Self {
            kind: TableCapabilityKind::Delete,
            row_type: TypeId::of::<T::Row>(),
            change_registration: register_channel::<TableChange<T::Row>>,
            callbacks: vec![(
                RowCallback::Delete,
                Arc::new(|world, db| {
                    bind_delete(world, &T::get(db));
                }),
            )],
            _marker: PhantomData,
        }
    }

    /// Binds update messages from `T`.
    pub fn update() -> Self
    where
        T: TableAccessor<C::DbView> + Send + Sync + 'static,
        T::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<T::Row>: Send + Sync,
        for<'db> T::Handle<'db>: TableLike<
                Row = T::Row,
                EventContext = <<T::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithUpdate,
    {
        Self {
            kind: TableCapabilityKind::Update,
            row_type: TypeId::of::<T::Row>(),
            change_registration: register_channel::<TableChange<T::Row>>,
            callbacks: vec![(
                RowCallback::Update,
                Arc::new(|world, db| {
                    bind_update(world, &T::get(db));
                }),
            )],
            _marker: PhantomData,
        }
    }

    /// Binds the insert-update reader for `T`.
    ///
    /// This reader requires both insert and update capabilities on the
    /// generated table handle.
    pub fn insert_update() -> Self
    where
        T: TableAccessor<C::DbView> + Send + Sync + 'static,
        T::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<T::Row>: Send + Sync,
        for<'db> T::Handle<'db>: TableLike<
                Row = T::Row,
                EventContext = <<T::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithInsert
            + WithUpdate,
    {
        Self {
            kind: TableCapabilityKind::InsertUpdate,
            row_type: TypeId::of::<T::Row>(),
            change_registration: register_channel::<TableChange<T::Row>>,
            callbacks: vec![
                (
                    RowCallback::Insert,
                    Arc::new(|world, db| {
                        bind_insert(world, &T::get(db));
                    }),
                ),
                (
                    RowCallback::Update,
                    Arc::new(|world, db| {
                        bind_update(world, &T::get(db));
                    }),
                ),
            ],
            _marker: PhantomData,
        }
    }

    pub(crate) fn register(self, registry: &mut TableRegistry<C, M>)
    where
        T: 'static,
    {
        registry.register_capability::<T>(
            self.kind,
            self.row_type,
            self.change_registration,
            self.callbacks,
        );
    }
}

impl<C, M> TableRegistry<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    pub(crate) fn bind<TTable>(
        &mut self,
        capabilities: impl IntoIterator<Item = TableCapability<C, M, TTable>>,
    ) where
        TTable: 'static,
    {
        for capability in capabilities {
            capability.register(self);
        }
    }

    fn register_capability<TTable>(
        &mut self,
        kind: TableCapabilityKind,
        row_type: TypeId,
        change_register: fn(&mut bevy_app::App),
        callbacks: Vec<(RowCallback, Arc<TableBindCallback<C>>)>,
    ) where
        TTable: 'static,
    {
        let accessor = TypeId::of::<TTable>();
        self.ledger
            .claim_capability(accessor, kind, type_name::<TTable>());

        // The channel is keyed by row type, not by accessor. A table and a view over one row
        // (`monster_instance_tbl` and `monster_instance_aoi`) share it, and `register_channel`
        // panics on a second registration of the same message type.
        if self.ledger.claim_change_channel(row_type) {
            self.table_registrations.push(Arc::new(change_register));
        }
        self.ledger.claim_stream(row_type, kind);

        // Callbacks are per accessor: each one feeds the shared channel from its own table.
        for (callback, bind) in callbacks {
            if self.ledger.claim_callback(accessor, callback) {
                self.table_bindings.push(bind);
            }
        }
    }
}

/// Tracks which accessor/capability pairs and shared change channels a [`TableRegistry`] has
/// already registered, so each underlying channel is registered exactly once.
#[derive(Default)]
pub(crate) struct CapabilityLedger {
    /// Claimed accessor/capability pairs, for duplicate detection.
    capabilities: Vec<(TypeId, TableCapabilityKind)>,
    /// Row/capability pairs a reader may read, recorded so an unbound one panics precisely.
    streams: Vec<(TypeId, TableCapabilityKind)>,
    /// Row types whose shared [`TableChange`] channel is registered.
    change_channels: Vec<TypeId>,
    /// Accessor/callback pairs already bound on the SDK table handle.
    callbacks: Vec<(TypeId, RowCallback)>,
}

impl CapabilityLedger {
    /// Records an accessor/capability pair.
    ///
    /// # Panics
    ///
    /// Panics if the pair was already claimed. `accessor_name` names it in the message.
    fn claim_capability(
        &mut self,
        accessor: TypeId,
        kind: TableCapabilityKind,
        accessor_name: &str,
    ) {
        let key = (accessor, kind);
        assert!(
            !self.capabilities.contains(&key),
            "duplicate table capability registration: accessor `{accessor_name}` already has `{kind:?}` bound",
        );
        self.capabilities.push(key);
    }

    /// Records that a reader of `kind` over `row_type` has the callbacks it needs.
    ///
    /// Idempotent: a table and a view over one row both claim it, and the reader only asks
    /// whether anyone did.
    fn claim_stream(&mut self, row_type: TypeId, kind: TableCapabilityKind) {
        let key = (row_type, kind);
        if !self.streams.contains(&key) {
            self.streams.push(key);
        }
    }

    /// Returns the readable streams as the resource the table readers check.
    pub(crate) fn bound_streams(&self) -> BoundStreams {
        BoundStreams {
            streams: self.streams.clone(),
        }
    }

    /// Claims an SDK row callback on `accessor`, returning whether this caller is the first to
    /// do so and must therefore bind it.
    fn claim_callback(&mut self, accessor: TypeId, callback: RowCallback) -> bool {
        let key = (accessor, callback);
        if self.callbacks.contains(&key) {
            return false;
        }
        self.callbacks.push(key);
        true
    }

    /// Claims the shared [`TableChange`] channel for `row_type`, returning whether this caller is
    /// the first to do so and must therefore register it.
    fn claim_change_channel(&mut self, row_type: TypeId) -> bool {
        if self.change_channels.contains(&row_type) {
            return false;
        }
        self.change_channels.push(row_type);
        true
    }
}

/// The row/capability pairs the plugin bound, so a table reader can name the missing
/// registration instead of yielding nothing forever.
///
/// Public only because the reader types are; it carries no useful API of its own.
#[doc(hidden)]
#[derive(Resource, Default, Clone)]
pub struct BoundStreams {
    streams: Vec<(TypeId, TableCapabilityKind)>,
}

impl BoundStreams {
    /// # Panics
    ///
    /// Panics unless a capability of `kind` was bound for `TRow`.
    pub fn assert_bound<TRow: 'static>(&self, kind: TableCapabilityKind) {
        assert!(
            self.streams.contains(&(TypeId::of::<TRow>(), kind)),
            "no `{kind:?}` capability is bound for row type `{}`; register it with the matching \
             `StdbPlugin` method before reading it",
            type_name::<TRow>(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{CapabilityLedger, RowCallback, TableCapabilityKind};
    use std::any::TypeId;

    // Stand-ins for a generated table accessor, a view accessor over the same row, and two rows.
    struct MonsterTbl;
    struct MonsterAoi;
    struct MonsterRow;
    struct PlayerRow;

    fn claim(ledger: &mut CapabilityLedger, accessor: TypeId, kind: TableCapabilityKind) {
        ledger.claim_capability(accessor, kind, "TestAccessor");
    }

    #[test]
    fn the_first_capability_of_any_kind_claims_the_change_channel() {
        // Every capability registers the channel, so whichever is claimed first must be the one
        // that registers it and the rest must not -- `register_channel` panics on a duplicate,
        // and a row whose channel is never registered panics with "unregistered channel" when a
        // connection binds it.
        for first in [
            TableCapabilityKind::InsertUpdate,
            TableCapabilityKind::Insert,
            TableCapabilityKind::Delete,
            TableCapabilityKind::Update,
        ] {
            let mut ledger = CapabilityLedger::default();
            let row = TypeId::of::<MonsterRow>();

            claim(&mut ledger, TypeId::of::<MonsterTbl>(), first);
            assert!(ledger.claim_change_channel(row), "first was {first:?}");

            claim(
                &mut ledger,
                TypeId::of::<MonsterAoi>(),
                TableCapabilityKind::Insert,
            );
            assert!(!ledger.claim_change_channel(row), "first was {first:?}");
        }
    }

    #[test]
    fn a_table_and_a_view_over_one_row_share_their_channel() {
        // `register_channel` panics on a duplicate message type, and message types are keyed by
        // row, so the second accessor over `MonsterRow` must claim nothing.
        let mut ledger = CapabilityLedger::default();
        let (row, kind) = (TypeId::of::<MonsterRow>(), TableCapabilityKind::Insert);

        claim(&mut ledger, TypeId::of::<MonsterTbl>(), kind);
        assert!(ledger.claim_change_channel(row));
        ledger.claim_stream(row, kind);

        claim(&mut ledger, TypeId::of::<MonsterAoi>(), kind);
        assert!(!ledger.claim_change_channel(row));
        ledger.claim_stream(row, kind);

        // One accessor is enough to make the stream readable, and the second must not duplicate
        // the entry.
        assert_eq!(ledger.streams.len(), 1);
        ledger.bound_streams().assert_bound::<MonsterRow>(kind);
    }

    #[test]
    fn every_capability_shares_one_change_channel() {
        let mut ledger = CapabilityLedger::default();
        let row = TypeId::of::<MonsterRow>();

        for kind in [
            TableCapabilityKind::Insert,
            TableCapabilityKind::Delete,
            TableCapabilityKind::Update,
            TableCapabilityKind::InsertUpdate,
        ] {
            ledger.claim_stream(row, kind);
        }
        assert!(ledger.claim_change_channel(row));
        assert!(!ledger.claim_change_channel(row));

        let bound = ledger.bound_streams();
        for kind in [
            TableCapabilityKind::Insert,
            TableCapabilityKind::Delete,
            TableCapabilityKind::Update,
            TableCapabilityKind::InsertUpdate,
        ] {
            bound.assert_bound::<MonsterRow>(kind);
        }
    }

    #[test]
    #[should_panic(expected = "no `Update` capability is bound")]
    fn reading_a_stream_nobody_bound_says_which_one() {
        // Readers filter the shared stream, so an unbound capability would otherwise read as a
        // table that never changes.
        let mut ledger = CapabilityLedger::default();
        ledger.claim_stream(TypeId::of::<MonsterRow>(), TableCapabilityKind::Insert);

        ledger
            .bound_streams()
            .assert_bound::<MonsterRow>(TableCapabilityKind::Update);
    }

    #[test]
    fn a_callback_needed_by_two_capabilities_is_bound_once() {
        // `add_table` binds `Insert` and `InsertUpdate`, which both need `on_insert`. Binding it
        // twice would forward every inserted row into the shared channel twice.
        let mut ledger = CapabilityLedger::default();
        let tbl = TypeId::of::<MonsterTbl>();

        assert!(ledger.claim_callback(tbl, RowCallback::Insert));
        assert!(!ledger.claim_callback(tbl, RowCallback::Insert));
        assert!(ledger.claim_callback(tbl, RowCallback::Update));
    }

    #[test]
    fn each_accessor_binds_its_own_callbacks() {
        // A table and a view over one row are different SDK tables: both must be bound, even
        // though they share the downstream channel.
        let mut ledger = CapabilityLedger::default();

        assert!(ledger.claim_callback(TypeId::of::<MonsterTbl>(), RowCallback::Insert));
        assert!(ledger.claim_callback(TypeId::of::<MonsterAoi>(), RowCallback::Insert));
    }

    #[test]
    fn distinct_row_types_claim_distinct_channels() {
        let mut ledger = CapabilityLedger::default();

        assert!(ledger.claim_change_channel(TypeId::of::<MonsterRow>()));
        assert!(ledger.claim_change_channel(TypeId::of::<PlayerRow>()));
    }

    #[test]
    #[should_panic(expected = "already has `Insert` bound")]
    fn one_accessor_may_not_claim_a_capability_twice() {
        let mut ledger = CapabilityLedger::default();

        claim(
            &mut ledger,
            TypeId::of::<MonsterTbl>(),
            TableCapabilityKind::Insert,
        );
        claim(
            &mut ledger,
            TypeId::of::<MonsterTbl>(),
            TableCapabilityKind::Insert,
        );
    }
}

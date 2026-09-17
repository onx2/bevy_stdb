use super::{
    TableBindCallback, TableRegistry, bind_delete, bind_insert, bind_insert_update, bind_update,
};
use crate::{
    channel_bridge::register_channel,
    message::{
        DeleteMessage, InsertMessage, InsertUpdateMessage, RowEvent, TableChange, UpdateMessage,
    },
};
use spacetimedb_sdk::__codegen::{
    DbConnection, DbContext, InModule, SpacetimeModule, TableAccessor, TableLike, WithDelete,
    WithInsert, WithUpdate,
};
use std::{
    any::{TypeId, type_name},
    marker::PhantomData,
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableCapabilityKind {
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
    /// The row type this capability yields, which keys the shared change channel.
    row_type: TypeId,
    app_registration: fn(&mut bevy_app::App),
    change_registration: Option<fn(&mut bevy_app::App)>,
    table_binding: Arc<TableBindCallback<C>>,
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
            app_registration: register_channel::<InsertMessage<T::Row>>,
            change_registration: Some(register_channel::<TableChange<T::Row>>),
            table_binding: Arc::new(|world, db| {
                bind_insert(world, &T::get(db));
            }),
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
            app_registration: register_channel::<DeleteMessage<T::Row>>,
            change_registration: Some(register_channel::<TableChange<T::Row>>),
            table_binding: Arc::new(|world, db| {
                bind_delete(world, &T::get(db));
            }),
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
            app_registration: register_channel::<UpdateMessage<T::Row>>,
            change_registration: Some(register_channel::<TableChange<T::Row>>),
            table_binding: Arc::new(|world, db| {
                bind_update(world, &T::get(db));
            }),
            _marker: PhantomData,
        }
    }

    /// Binds insert-update messages from `T`.
    ///
    /// This derived message stream requires both insert and update
    /// capabilities on the generated table handle.
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
            app_registration: register_channel::<InsertUpdateMessage<T::Row>>,
            change_registration: None,
            table_binding: Arc::new(|world, db| {
                bind_insert_update(world, &T::get(db));
            }),
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
            self.app_registration,
            self.change_registration,
            self.table_binding,
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
        register: fn(&mut bevy_app::App),
        change_register: Option<fn(&mut bevy_app::App)>,
        bind: Arc<TableBindCallback<C>>,
    ) where
        TTable: 'static,
    {
        self.ledger
            .claim_capability(TypeId::of::<TTable>(), kind, type_name::<TTable>());

        // Channels are keyed by message type, so by row type -- not by accessor. A table and a
        // view over one row (`monster_instance_tbl` and `monster_instance_aoi`) share their
        // channels, and `register_channel` panics on a second registration of the same message
        // type. Each accessor still binds its own SDK callbacks, which feed the shared channel.
        if let Some(change_register) = change_register
            && self.ledger.claim_change_channel(row_type)
        {
            self.table_registrations.push(Arc::new(change_register));
        }
        if self.ledger.claim_channel(row_type, kind) {
            self.table_registrations.push(Arc::new(register));
        }
        self.table_bindings.push(bind);
    }
}

/// Tracks which accessor/capability pairs and shared change channels a [`TableRegistry`] has
/// already registered, so each underlying channel is registered exactly once.
#[derive(Default)]
pub(crate) struct CapabilityLedger {
    /// Claimed accessor/capability pairs, for duplicate detection.
    capabilities: Vec<(TypeId, TableCapabilityKind)>,
    /// Row/capability pairs whose typed channel is registered.
    channels: Vec<(TypeId, TableCapabilityKind)>,
    /// Row types whose shared [`TableChange`] channel is registered.
    change_channels: Vec<TypeId>,
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

    /// Claims the typed channel for `row_type` and `kind`, returning whether this caller is the
    /// first to do so and must therefore register it.
    fn claim_channel(&mut self, row_type: TypeId, kind: TableCapabilityKind) -> bool {
        let key = (row_type, kind);
        if self.channels.contains(&key) {
            return false;
        }
        self.channels.push(key);
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

#[cfg(test)]
mod tests {
    use super::{CapabilityLedger, TableCapabilityKind};
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
    fn insert_update_first_still_registers_the_change_channel() {
        // `bind` takes capabilities in caller order, so `InsertUpdate` can be claimed first even
        // though it never sends a `TableChange`. A later `Insert` must still register the
        // channel, or binding it panics with "unregistered channel" on connect.
        let mut ledger = CapabilityLedger::default();
        claim(
            &mut ledger,
            TypeId::of::<MonsterTbl>(),
            TableCapabilityKind::InsertUpdate,
        );
        claim(
            &mut ledger,
            TypeId::of::<MonsterTbl>(),
            TableCapabilityKind::Insert,
        );

        assert!(ledger.claim_change_channel(TypeId::of::<MonsterRow>()));
    }

    #[test]
    fn a_table_and_a_view_over_one_row_share_their_channels() {
        // `register_channel` panics on a duplicate message type, and message types are keyed by
        // row, so the second accessor over `MonsterRow` must claim nothing.
        let mut ledger = CapabilityLedger::default();
        let (row, kind) = (TypeId::of::<MonsterRow>(), TableCapabilityKind::Insert);

        claim(&mut ledger, TypeId::of::<MonsterTbl>(), kind);
        assert!(ledger.claim_change_channel(row));
        assert!(ledger.claim_channel(row, kind));

        claim(&mut ledger, TypeId::of::<MonsterAoi>(), kind);
        assert!(!ledger.claim_change_channel(row));
        assert!(!ledger.claim_channel(row, kind));
    }

    #[test]
    fn each_capability_claims_its_own_typed_channel() {
        let mut ledger = CapabilityLedger::default();
        let row = TypeId::of::<MonsterRow>();

        for kind in [
            TableCapabilityKind::Insert,
            TableCapabilityKind::Delete,
            TableCapabilityKind::Update,
            TableCapabilityKind::InsertUpdate,
        ] {
            assert!(ledger.claim_channel(row, kind), "{kind:?} channel");
        }
        // ...but they share one change channel.
        assert!(ledger.claim_change_channel(row));
        assert!(!ledger.claim_change_channel(row));
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

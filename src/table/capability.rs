use super::{
    TableBindCallback, TableRegistry, bind_delete, bind_insert, bind_insert_update, bind_update,
    register_delete, register_insert, register_insert_update, register_update,
};
use crate::message::RowEvent;
use spacetimedb_sdk::__codegen::{
    DbConnection, DbContext, InModule, SpacetimeModule, TableAccessor, TableLike, WithDelete,
    WithInsert, WithUpdate,
};
use std::{
    any::{TypeId, type_name},
    marker::PhantomData,
    sync::Arc,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableCapabilityKind {
    Insert,
    Delete,
    Update,
    InsertUpdate,
}

impl TableCapabilityKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Delete => "delete",
            Self::Update => "update",
            Self::InsertUpdate => "insert_update",
        }
    }
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
    register: fn(&mut TableRegistry<C, M>),
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
            register: register_insert_capability::<C, M, T>,
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
            register: register_delete_capability::<C, M, T>,
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
            register: register_update_capability::<C, M, T>,
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
            register: register_insert_update_capability::<C, M, T>,
            _marker: PhantomData,
        }
    }

    pub(crate) fn register(self, registry: &mut TableRegistry<C, M>) {
        (self.register)(registry);
    }
}

fn register_insert_capability<C, M, T>(registry: &mut TableRegistry<C, M>)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
    T: TableAccessor<C::DbView> + Send + Sync + 'static,
    T::Row: Send + Sync + Clone + InModule + 'static,
    RowEvent<T::Row>: Send + Sync,
    for<'db> T::Handle<'db>: TableLike<
            Row = T::Row,
            EventContext = <<T::Row as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithInsert,
{
    registry.bind_insert_capability::<T>();
}

fn register_delete_capability<C, M, T>(registry: &mut TableRegistry<C, M>)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
    T: TableAccessor<C::DbView> + Send + Sync + 'static,
    T::Row: Send + Sync + Clone + InModule + 'static,
    RowEvent<T::Row>: Send + Sync,
    for<'db> T::Handle<'db>: TableLike<
            Row = T::Row,
            EventContext = <<T::Row as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithDelete,
{
    registry.bind_delete_capability::<T>();
}

fn register_update_capability<C, M, T>(registry: &mut TableRegistry<C, M>)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
    T: TableAccessor<C::DbView> + Send + Sync + 'static,
    T::Row: Send + Sync + Clone + InModule + 'static,
    RowEvent<T::Row>: Send + Sync,
    for<'db> T::Handle<'db>: TableLike<
            Row = T::Row,
            EventContext = <<T::Row as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithUpdate,
{
    registry.bind_update_capability::<T>();
}

fn register_insert_update_capability<C, M, T>(registry: &mut TableRegistry<C, M>)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
    T: TableAccessor<C::DbView> + Send + Sync + 'static,
    T::Row: Send + Sync + Clone + InModule + 'static,
    RowEvent<T::Row>: Send + Sync,
    for<'db> T::Handle<'db>: TableLike<
            Row = T::Row,
            EventContext = <<T::Row as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithInsert
        + WithUpdate,
{
    registry.bind_insert_update_capability::<T>();
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
        register: fn(&mut bevy_app::App),
        bind: Arc<TableBindCallback<C>>,
    ) where
        TTable: 'static,
    {
        let key = (TypeId::of::<TTable>(), kind);
        assert!(
            !self.registered_capabilities.contains(&key),
            "duplicate table capability registration: accessor `{}` already has `{}` bound",
            type_name::<TTable>(),
            kind.name(),
        );
        self.registered_capabilities.push(key);
        self.table_registrations.push(Arc::new(register));
        self.table_bindings.push(bind);
    }

    pub(crate) fn bind_insert<TTable>(&mut self)
    where
        TTable: TableAccessor<C::DbView> + Send + Sync + 'static,
        TTable::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<TTable::Row>: Send + Sync,
        for<'db> TTable::Handle<'db>: TableLike<
                Row = TTable::Row,
                EventContext = <<TTable::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithInsert,
    {
        self.bind::<TTable>([TableCapability::insert()]);
    }

    fn bind_insert_capability<TTable>(&mut self)
    where
        TTable: TableAccessor<C::DbView> + Send + Sync + 'static,
        TTable::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<TTable::Row>: Send + Sync,
        for<'db> TTable::Handle<'db>: TableLike<
                Row = TTable::Row,
                EventContext = <<TTable::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithInsert,
    {
        self.register_capability::<TTable>(
            TableCapabilityKind::Insert,
            register_insert::<TTable::Row>,
            Arc::new(|world, db| {
                let table = TTable::get(db);
                bind_insert::<TTable::Row, _>(world, &table);
            }),
        );
    }

    pub(crate) fn bind_delete<TTable>(&mut self)
    where
        TTable: TableAccessor<C::DbView> + Send + Sync + 'static,
        TTable::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<TTable::Row>: Send + Sync,
        for<'db> TTable::Handle<'db>: TableLike<
                Row = TTable::Row,
                EventContext = <<TTable::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithDelete,
    {
        self.bind::<TTable>([TableCapability::delete()]);
    }

    fn bind_delete_capability<TTable>(&mut self)
    where
        TTable: TableAccessor<C::DbView> + Send + Sync + 'static,
        TTable::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<TTable::Row>: Send + Sync,
        for<'db> TTable::Handle<'db>: TableLike<
                Row = TTable::Row,
                EventContext = <<TTable::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithDelete,
    {
        self.register_capability::<TTable>(
            TableCapabilityKind::Delete,
            register_delete::<TTable::Row>,
            Arc::new(|world, db| {
                let table = TTable::get(db);
                bind_delete::<TTable::Row, _>(world, &table);
            }),
        );
    }

    pub(crate) fn bind_update<TTable>(&mut self)
    where
        TTable: TableAccessor<C::DbView> + Send + Sync + 'static,
        TTable::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<TTable::Row>: Send + Sync,
        for<'db> TTable::Handle<'db>: TableLike<
                Row = TTable::Row,
                EventContext = <<TTable::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithUpdate,
    {
        self.bind::<TTable>([TableCapability::update()]);
    }

    fn bind_update_capability<TTable>(&mut self)
    where
        TTable: TableAccessor<C::DbView> + Send + Sync + 'static,
        TTable::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<TTable::Row>: Send + Sync,
        for<'db> TTable::Handle<'db>: TableLike<
                Row = TTable::Row,
                EventContext = <<TTable::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithUpdate,
    {
        self.register_capability::<TTable>(
            TableCapabilityKind::Update,
            register_update::<TTable::Row>,
            Arc::new(|world, db| {
                let table = TTable::get(db);
                bind_update::<TTable::Row, _>(world, &table);
            }),
        );
    }

    pub(crate) fn bind_insert_update<TTable>(&mut self)
    where
        TTable: TableAccessor<C::DbView> + Send + Sync + 'static,
        TTable::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<TTable::Row>: Send + Sync,
        for<'db> TTable::Handle<'db>: TableLike<
                Row = TTable::Row,
                EventContext = <<TTable::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithInsert
            + WithUpdate,
    {
        self.bind::<TTable>([TableCapability::insert_update()]);
    }

    fn bind_insert_update_capability<TTable>(&mut self)
    where
        TTable: TableAccessor<C::DbView> + Send + Sync + 'static,
        TTable::Row: Send + Sync + Clone + InModule + 'static,
        RowEvent<TTable::Row>: Send + Sync,
        for<'db> TTable::Handle<'db>: TableLike<
                Row = TTable::Row,
                EventContext = <<TTable::Row as InModule>::Module as SpacetimeModule>::EventContext,
            > + WithInsert
            + WithUpdate,
    {
        self.register_capability::<TTable>(
            TableCapabilityKind::InsertUpdate,
            register_insert_update::<TTable::Row>,
            Arc::new(|world, db| {
                let table = TTable::get(db);
                bind_insert_update::<TTable::Row, _>(world, &table);
            }),
        );
    }
}

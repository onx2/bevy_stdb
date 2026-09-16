use super::{
    TableBindCallback, TableRegistry, bind_change_delete, bind_change_insert, bind_change_update,
    bind_delete, bind_insert, bind_insert_update, bind_update,
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
    ChangeInsert,
    ChangeDelete,
    ChangeUpdate,
}

impl TableCapabilityKind {
    fn is_change(self) -> bool {
        matches!(
            self,
            Self::ChangeInsert | Self::ChangeDelete | Self::ChangeUpdate
        )
    }
}

/// A typed table binding capability used with [`crate::prelude::StdbPlugin::bind`].
///
/// Construct capabilities with [`Self::insert`], [`Self::delete`],
/// [`Self::update`], [`Self::insert_update`], [`Self::change_insert`],
/// [`Self::change_delete`], and [`Self::change_update`]. Each constructor
/// requires the corresponding capability trait on the generated table handle,
/// so unsupported bindings fail at compile time.
pub struct TableCapability<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
    T,
> {
    kind: TableCapabilityKind,
    app_registration: fn(&mut bevy_app::App),
    table_binding: Arc<TableBindCallback<C>>,
    _marker: PhantomData<fn() -> T>,
}

impl<C, M, T> TableCapability<C, M, T>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    /// Binds ordered insert changes from `T`.
    pub fn change_insert() -> Self
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
            kind: TableCapabilityKind::ChangeInsert,
            app_registration: register_channel::<TableChange<T::Row>>,
            table_binding: Arc::new(|world, db| {
                bind_change_insert(world, &T::get(db));
            }),
            _marker: PhantomData,
        }
    }

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
            app_registration: register_channel::<InsertMessage<T::Row>>,
            table_binding: Arc::new(|world, db| {
                bind_insert(world, &T::get(db));
            }),
            _marker: PhantomData,
        }
    }

    /// Binds ordered delete changes from `T`.
    pub fn change_delete() -> Self
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
            kind: TableCapabilityKind::ChangeDelete,
            app_registration: register_channel::<TableChange<T::Row>>,
            table_binding: Arc::new(|world, db| {
                bind_change_delete(world, &T::get(db));
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
            app_registration: register_channel::<DeleteMessage<T::Row>>,
            table_binding: Arc::new(|world, db| {
                bind_delete(world, &T::get(db));
            }),
            _marker: PhantomData,
        }
    }

    /// Binds ordered update changes from `T`.
    pub fn change_update() -> Self
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
            kind: TableCapabilityKind::ChangeUpdate,
            app_registration: register_channel::<TableChange<T::Row>>,
            table_binding: Arc::new(|world, db| {
                bind_change_update(world, &T::get(db));
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
            app_registration: register_channel::<UpdateMessage<T::Row>>,
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
            app_registration: register_channel::<InsertUpdateMessage<T::Row>>,
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
        registry.register_capability::<T>(self.kind, self.app_registration, self.table_binding);
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
        register: fn(&mut bevy_app::App),
        bind: Arc<TableBindCallback<C>>,
    ) where
        TTable: 'static,
    {
        let table_id = TypeId::of::<TTable>();
        let key = (table_id, kind);
        assert!(
            !self.registered_capabilities.contains(&key),
            "duplicate table capability registration: accessor `{}` already has `{:?}` bound",
            type_name::<TTable>(),
            kind,
        );
        let has_change_registration = self
            .registered_capabilities
            .iter()
            .any(|(table, capability)| *table == table_id && capability.is_change());
        self.registered_capabilities.push(key);

        if kind.is_change() {
            if !has_change_registration {
                self.table_registrations.push(Arc::new(register));
            }
        } else {
            self.table_registrations.push(Arc::new(register));
        }
        self.table_bindings.push(bind);
    }
}

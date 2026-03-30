//! Table registration for SpacetimeDB message forwarding.
//!
//! This module separates one-time Bevy message registration from runtime table
//! callback binding while keeping a single consumer-facing declaration per
//! table.

use crate::{
    channel_bridge::{channel_sender, register_channel},
    message::{DeleteMessage, InsertMessage, InsertUpdateMessage, UpdateMessage},
};
use bevy_app::App;
use bevy_ecs::world::World;
use spacetimedb_sdk::__codegen::DbContext;
use spacetimedb_sdk::{EventTable, Table, TableWithPrimaryKey};
use std::marker::PhantomData;
use std::sync::Arc;

/// Stored callback that performs one-time Bevy app registration for a table/view.
pub(crate) type TableRegistrationCallback = dyn Fn(&mut App) + Send + Sync;

/// Stored callback that binds SpacetimeDB table listeners for a concrete database view.
pub(crate) type TableBindCallback<C> =
    dyn for<'db> Fn(&World, &'db <C as DbContext>::DbView) + Send + Sync;

/// Helper passed to stored bind callbacks.
///
/// This is single-use by construction: terminal methods consume `self`, so a
/// single `with_table*` call can only bind one table.
pub struct TableBinder<'a, 'db, C>
where
    C: DbContext,
{
    world: &'a World,
    db: &'db C::DbView,
}

impl<'a, 'db, C> TableBinder<'a, 'db, C>
where
    C: DbContext,
{
    /// Creates a new binder for the active world and database view.
    pub(crate) fn new(world: &'a World, db: &'db C::DbView) -> Self {
        Self { world, db }
    }

    /// Returns the current database view.
    pub fn db(&self) -> &'db C::DbView {
        self.db
    }

    /// Binds a table with a primary key.
    ///
    /// This forwards:
    /// - [`InsertMessage`]
    /// - [`DeleteMessage`]
    /// - [`UpdateMessage`]
    /// - [`InsertUpdateMessage`]
    pub fn table<TRow, TTable>(self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow> + TableWithPrimaryKey<Row = TRow>,
    {
        bind_insert::<TRow, TTable>(self.world, table);
        bind_delete::<TRow, TTable>(self.world, table);
        bind_update::<TRow, TTable>(self.world, table);
        bind_insert_update::<TRow, TTable>(self.world, table);
    }

    /// Binds a table without a primary key.
    ///
    /// This forwards:
    /// - [`InsertMessage`]
    /// - [`DeleteMessage`]
    pub fn table_without_pk<TRow, TTable>(self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow>,
    {
        bind_insert::<TRow, TTable>(self.world, table);
        bind_delete::<TRow, TTable>(self.world, table);
    }

    /// Binds a view.
    ///
    /// This is equivalent to [`TableBinder::table_without_pk`].
    pub fn view<TRow, TTable>(self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow>,
    {
        self.table_without_pk::<TRow, TTable>(table);
    }

    /// Binds an event table.
    ///
    /// This forwards:
    /// - [`InsertMessage`]
    pub fn event_table<TRow, TTable>(self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow> + EventTable,
    {
        bind_insert::<TRow, TTable>(self.world, table);
    }
}

/// Builder-style registrar used during plugin configuration.
///
/// This records:
/// - one-time Bevy message registrations
/// - runtime binding callbacks that receive a live database view later
pub struct TableRegistrar<'a, C>
where
    C: DbContext,
{
    registrations: &'a mut Vec<Arc<TableRegistrationCallback>>,
    bindings: &'a mut Vec<Arc<TableBindCallback<C>>>,
}

impl<'a, C> TableRegistrar<'a, C>
where
    C: DbContext,
{
    /// Creates a new [`TableRegistrar`].
    pub fn new(
        registrations: &'a mut Vec<Arc<TableRegistrationCallback>>,
        bindings: &'a mut Vec<Arc<TableBindCallback<C>>>,
    ) -> Self {
        Self {
            registrations,
            bindings,
        }
    }

    /// Registers a table with a primary key using a single stored bind closure.
    pub fn table<TRow>(
        &mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
    ) where
        TRow: Send + Sync + Clone + 'static,
    {
        self.registrations.push(Arc::new(register_table::<TRow>));
        self.bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));
    }

    /// Registers a table without a primary key using a single stored bind closure.
    pub fn table_without_pk<TRow>(
        &mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
    ) where
        TRow: Send + Sync + Clone + 'static,
    {
        self.registrations
            .push(Arc::new(register_table_without_pk::<TRow>));
        self.bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));
    }

    /// Registers a view using a single stored bind closure.
    pub fn view<TRow>(
        &mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
    ) where
        TRow: Send + Sync + Clone + 'static,
    {
        self.registrations.push(Arc::new(register_view::<TRow>));
        self.bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));
    }

    /// Registers an event table using a single stored bind closure.
    pub fn event_table<TRow>(
        &mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
    ) where
        TRow: Send + Sync + Clone + 'static,
    {
        self.registrations
            .push(Arc::new(register_event_table::<TRow>));
        self.bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));
    }

    /// Uses [`TableRegistrationBuilder`] to select which Bevy messages to register
    /// while still receiving a single runtime bind closure.
    pub fn build<TRow>(
        &mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
        build: impl for<'r> FnOnce(&mut TableRegistrationBuilder<'r, TRow>),
    ) where
        TRow: Send + Sync + Clone + 'static,
    {
        let mut builder = TableRegistrationBuilder {
            registrations: self.registrations,
            _row: PhantomData,
        };
        build(&mut builder);

        self.bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));
    }
}

/// Builder for selecting which Bevy messages should be registered for a row type.
pub struct TableRegistrationBuilder<'a, TRow>
where
    TRow: Send + Sync + Clone + 'static,
{
    registrations: &'a mut Vec<Arc<TableRegistrationCallback>>,
    _row: PhantomData<TRow>,
}

impl<'a, TRow> TableRegistrationBuilder<'a, TRow>
where
    TRow: Send + Sync + Clone + 'static,
{
    /// Registers inserts as [`InsertMessage`].
    pub fn insert(&mut self) -> &mut Self {
        self.registrations
            .push(Arc::new(|app| register_channel::<InsertMessage<TRow>>(app)));
        self
    }

    /// Registers deletes as [`DeleteMessage`].
    pub fn delete(&mut self) -> &mut Self {
        self.registrations
            .push(Arc::new(|app| register_channel::<DeleteMessage<TRow>>(app)));
        self
    }

    /// Registers updates as [`UpdateMessage`].
    pub fn update(&mut self) -> &mut Self {
        self.registrations
            .push(Arc::new(|app| register_channel::<UpdateMessage<TRow>>(app)));
        self
    }

    /// Registers inserts and updates as [`InsertUpdateMessage`].
    pub fn insert_update(&mut self) -> &mut Self {
        self.registrations.push(Arc::new(|app| {
            register_channel::<InsertUpdateMessage<TRow>>(app)
        }));
        self
    }
}

/// Registers the Bevy messages for a table with a primary key.
pub(crate) fn register_table<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + 'static,
{
    register_channel::<InsertMessage<TRow>>(app);
    register_channel::<DeleteMessage<TRow>>(app);
    register_channel::<UpdateMessage<TRow>>(app);
    register_channel::<InsertUpdateMessage<TRow>>(app);
}

/// Registers the Bevy messages for a table without a primary key.
pub(crate) fn register_table_without_pk<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + 'static,
{
    register_channel::<InsertMessage<TRow>>(app);
    register_channel::<DeleteMessage<TRow>>(app);
}

/// Registers the Bevy messages for a view.
pub(crate) fn register_view<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + 'static,
{
    register_table_without_pk::<TRow>(app);
}

/// Registers the Bevy messages for an event table.
pub(crate) fn register_event_table<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + 'static,
{
    register_channel::<InsertMessage<TRow>>(app);
}

pub(crate) fn bind_insert<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
{
    let sender = channel_sender::<InsertMessage<TRow>>(world);
    table.on_insert(move |_ctx, row| {
        let _ = sender.send(InsertMessage { row: row.clone() });
    });
}

pub(crate) fn bind_delete<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
{
    let sender = channel_sender::<DeleteMessage<TRow>>(world);
    table.on_delete(move |_ctx, row| {
        let _ = sender.send(DeleteMessage { row: row.clone() });
    });
}

pub(crate) fn bind_update<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow> + TableWithPrimaryKey<Row = TRow>,
{
    let sender = channel_sender::<UpdateMessage<TRow>>(world);
    table.on_update(move |_ctx, old, new| {
        let _ = sender.send(UpdateMessage {
            old: old.clone(),
            new: new.clone(),
        });
    });
}

pub(crate) fn bind_insert_update<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow> + TableWithPrimaryKey<Row = TRow>,
{
    let sender_insert = channel_sender::<InsertUpdateMessage<TRow>>(world);
    table.on_insert(move |_ctx, row| {
        let _ = sender_insert.send(InsertUpdateMessage {
            old: None,
            new: row.clone(),
        });
    });

    let sender_update = channel_sender::<InsertUpdateMessage<TRow>>(world);
    table.on_update(move |_ctx, old, new| {
        let _ = sender_update.send(InsertUpdateMessage {
            old: Some(old.clone()),
            new: new.clone(),
        });
    });
}

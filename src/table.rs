//! Table registration for SpacetimeDB message forwarding.
//!
//! This module binds table callbacks and forwards table changes into Bevy messages.

use crate::{
    channel_bridge::{channel_sender, register_channel},
    message::{DeleteMessage, InsertMessage, InsertUpdateMessage, UpdateMessage},
};
use bevy_app::App;
use bevy_ecs::world::World;
use spacetimedb_sdk::__codegen::DbContext;
use spacetimedb_sdk::{EventTable, Table, TableWithPrimaryKey};
use std::marker::PhantomData;

pub(crate) type TableRegistrarCallback<C> =
    dyn for<'a, 'db> Fn(&mut TableRegistrar<'a>, &'db <C as DbContext>::DbView) + Send + Sync;

pub(crate) enum RegistrarMode<'a> {
    Init(&'a mut App),
    Bind(&'a World),
}

/// Registers SpacetimeDB table callbacks as Bevy messages.
///
/// Registration runs once to initialize channels and again to bind callbacks
/// for the active connection.
pub struct TableRegistrar<'a> {
    mode: RegistrarMode<'a>,
}

impl<'a> TableRegistrar<'a> {
    /// Creates a new [`TableRegistrar`] with the given mode.
    pub(crate) fn new(mode: RegistrarMode<'a>) -> Self {
        Self { mode }
    }

    /// Registers a table with a primary key.
    ///
    /// Forwards table changes as:
    /// - [`InsertMessage`], [`DeleteMessage`], [`UpdateMessage`], and [`InsertUpdateMessage`]
    pub fn table<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow> + TableWithPrimaryKey<Row = TRow>,
    {
        self.build(table, |table| {
            table.insert();
            table.delete();
            table.update();
            table.insert_update();
        });
    }

    /// Registers a table without a primary key.
    ///
    /// Forwards inserts and deletes as [`InsertMessage`] and [`DeleteMessage`].
    pub fn table_without_pk<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow>,
    {
        self.build(table, |table| {
            table.insert();
            table.delete();
        });
    }

    /// Registers a view.
    ///
    /// This is equivalent to [`TableRegistrar::table_without_pk`].
    pub fn view<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow>,
    {
        self.table_without_pk(table);
    }

    /// Registers an event table.
    ///
    /// Forwards inserts as [`InsertMessage`].
    pub fn event_table<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow> + EventTable,
    {
        self.build(table, |table| {
            table.insert();
        });
    }

    /// Use [`TableBindingBuilder`] to select which messages to forward.
    pub fn build<TRow, TTable>(
        &mut self,
        table: &TTable,
        build: impl for<'r> FnOnce(&mut TableBindingBuilder<'r, 'a, TRow, TTable>),
    ) where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow>,
    {
        build(&mut TableBindingBuilder {
            registrar: self,
            table,
            _row: PhantomData,
        });
    }
}

/// Builder for configuring which table events should be forwarded.
pub struct TableBindingBuilder<'r, 't, TRow, TTable>
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
{
    registrar: &'r mut TableRegistrar<'t>,
    table: &'r TTable,
    _row: PhantomData<TRow>,
}

impl<'r, 't, TRow, TTable> TableBindingBuilder<'r, 't, TRow, TTable>
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
{
    /// Forwards inserts as [`InsertMessage`].
    pub fn insert(&mut self) -> &mut Self {
        match &mut self.registrar.mode {
            RegistrarMode::Init(app) => register_channel::<InsertMessage<TRow>>(app),
            RegistrarMode::Bind(world) => bind_insert::<TRow, TTable>(world, self.table),
        }
        self
    }
}

impl<'r, 't, TRow, TTable> TableBindingBuilder<'r, 't, TRow, TTable>
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
    TTable: TableWithPrimaryKey<Row = TRow>,
{
    /// Forwards updates as [`UpdateMessage`].
    pub fn update(&mut self) -> &mut Self {
        match &mut self.registrar.mode {
            RegistrarMode::Init(app) => register_channel::<UpdateMessage<TRow>>(app),
            RegistrarMode::Bind(world) => bind_update::<TRow, TTable>(world, self.table),
        }
        self
    }

    /// Forwards inserts and updates as [`InsertUpdateMessage`].
    pub fn insert_update(&mut self) -> &mut Self {
        match &mut self.registrar.mode {
            RegistrarMode::Init(app) => register_channel::<InsertUpdateMessage<TRow>>(app),
            RegistrarMode::Bind(world) => bind_insert_update::<TRow, TTable>(world, self.table),
        }
        self
    }
}

impl<'r, 't, TRow, TTable> TableBindingBuilder<'r, 't, TRow, TTable>
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
{
    /// Forwards deletes as [`DeleteMessage`].
    pub fn delete(&mut self) -> &mut Self {
        match &mut self.registrar.mode {
            RegistrarMode::Init(app) => register_channel::<DeleteMessage<TRow>>(app),
            RegistrarMode::Bind(world) => bind_delete::<TRow, TTable>(world, self.table),
        }
        self
    }
}

fn bind_insert<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
{
    let sender = channel_sender::<InsertMessage<TRow>>(world);
    table.on_insert(move |_ctx, row| {
        let _ = sender.send(InsertMessage { row: row.clone() });
    });
}

fn bind_delete<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
{
    let sender = channel_sender::<DeleteMessage<TRow>>(world);
    table.on_delete(move |_ctx, row| {
        let _ = sender.send(DeleteMessage { row: row.clone() });
    });
}

fn bind_update<TRow, TTable>(world: &World, table: &TTable)
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

fn bind_insert_update<TRow, TTable>(world: &World, table: &TTable)
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

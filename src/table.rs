//! Table registration for SpacetimeDB message forwarding.
//!
//! This module binds table callbacks and forwards table changes into Bevy messages.

use crate::{
    channel_bridge::{channel_sender, register_channel},
    message::{DeleteMessage, InsertMessage, InsertUpdateMessage, UpdateMessage},
};
use bevy_app::App;
use bevy_ecs::{message::Message, world::World};
use spacetimedb_sdk::__codegen::DbContext;
use spacetimedb_sdk::{EventTable, Table, TableWithPrimaryKey};
use std::{marker::PhantomData, sync::mpsc::Sender};

pub trait NonEventTable {}
impl<T> NonEventTable for T where T: Table + ?Sized {}

pub(crate) type TableRegistrarCallback<C> =
    dyn for<'a, 'db> Fn(&mut TableRegistrar<'a>, &'db <C as DbContext>::DbView) + Send + Sync;

/// Registers SpacetimeDB table callbacks as Bevy messages.
///
/// Registration runs once to initialize channels and again to bind callbacks
/// for the active connection.
pub struct TableRegistrar<'a> {
    mode: TableRegistrarMode<'a>,
}

enum TableRegistrarMode<'a> {
    Init(&'a mut App),
    Bind(&'a World),
}

/// Builder for configuring which table events should be forwarded.
///
/// Base methods available for all tables:
/// - [`TableBindingBuilder::insert`]
/// - [`TableBindingBuilder::delete`]
///
/// Additional methods available only for tables with a primary key:
/// - [`PkTableBindingBuilder::update`]
/// - [`PkTableBindingBuilder::insert_update`]
pub struct TableBindingBuilder<'r, 't, TRow, TTable>
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
{
    registrar: &'r mut TableRegistrar<'t>,
    table: &'r TTable,
    _row: PhantomData<TRow>,
}

impl<'a> TableRegistrar<'a> {
    /// Creates a registrar that initializes message channels.
    pub fn new_init(app: &'a mut App) -> Self {
        Self {
            mode: TableRegistrarMode::Init(app),
        }
    }

    /// Creates a registrar that binds callbacks for the active connection.
    pub fn new_bind(world: &'a World) -> Self {
        Self {
            mode: TableRegistrarMode::Bind(world),
        }
    }

    /// Returns the init-phase app.
    fn expect_init(&mut self) -> &mut App {
        match &mut self.mode {
            TableRegistrarMode::Init(app) => app,
            _ => panic!("table registration is only valid during table init"),
        }
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

    /// Registers a table using a configurable builder.
    ///
    /// Base bindings available for all tables:
    /// - [`TableBindingBuilder::insert`]
    ///
    /// Additional bindings become available when the table satisfies the
    /// required trait bounds:
    /// - [`TableBindingBuilder::delete`] for non-event tables
    /// - [`TableBindingBuilder::update`] for tables with a primary key
    /// - [`TableBindingBuilder::insert_update`] for tables with a primary key
    pub fn build<TRow, TTable>(
        &mut self,
        table: &TTable,
        build: impl for<'r> FnOnce(&mut TableBindingBuilder<'r, 'a, TRow, TTable>),
    ) where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow>,
    {
        let mut builder = TableBindingBuilder {
            registrar: self,
            table,
            _row: PhantomData,
        };

        build(&mut builder);
    }

    /// Returns the sender for the given message type during runtime binding.
    fn sender<T>(&mut self) -> Sender<T>
    where
        T: Message,
    {
        match &mut self.mode {
            TableRegistrarMode::Init(_) => {
                panic!("sender lookup is only valid during runtime table binding")
            }
            TableRegistrarMode::Bind(world) => channel_sender::<T>(world),
        }
    }

    /// Binds insert forwarding for the given table.
    fn bind_insert<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow>,
    {
        let sender = self.sender::<InsertMessage<TRow>>();
        table.on_insert(move |_ctx, row| {
            let _ = sender.send(InsertMessage { row: row.clone() });
        });
    }

    /// Binds delete forwarding for the given table.
    fn bind_delete<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow>,
    {
        let sender = self.sender::<DeleteMessage<TRow>>();
        table.on_delete(move |_ctx, row| {
            let _ = sender.send(DeleteMessage { row: row.clone() });
        });
    }

    /// Binds update forwarding for the given table.
    fn bind_update<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow> + TableWithPrimaryKey<Row = TRow>,
    {
        let sender = self.sender::<UpdateMessage<TRow>>();
        table.on_update(move |_ctx, old, new| {
            let _ = sender.send(UpdateMessage {
                old: old.clone(),
                new: new.clone(),
            });
        });
    }

    /// Binds insert-or-update forwarding for the given table.
    fn bind_insert_update<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow> + TableWithPrimaryKey<Row = TRow>,
    {
        let sender_insert = self.sender::<InsertUpdateMessage<TRow>>();
        let sender_update = self.sender::<InsertUpdateMessage<TRow>>();

        table.on_insert(move |_ctx, row| {
            let _ = sender_insert.send(InsertUpdateMessage {
                old: None,
                new: row.clone(),
            });
        });

        table.on_update(move |_ctx, old, new| {
            let _ = sender_update.send(InsertUpdateMessage {
                old: Some(old.clone()),
                new: new.clone(),
            });
        });
    }
}

impl<'r, 't, TRow, TTable> TableBindingBuilder<'r, 't, TRow, TTable>
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow>,
{
    /// Forwards inserts as [`InsertMessage`].
    pub fn insert(&mut self) -> &mut Self {
        match self.registrar.mode {
            TableRegistrarMode::Init(_) => {
                register_channel::<InsertMessage<TRow>>(self.registrar.expect_init());
            }
            TableRegistrarMode::Bind(_) => self.registrar.bind_insert::<TRow, TTable>(self.table),
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
        match self.registrar.mode {
            TableRegistrarMode::Init(_) => {
                register_channel::<UpdateMessage<TRow>>(self.registrar.expect_init());
            }
            TableRegistrarMode::Bind(_) => self.registrar.bind_update::<TRow, TTable>(self.table),
        }
        self
    }

    /// Forwards inserts and updates as [`InsertUpdateMessage`].
    pub fn insert_update(&mut self) -> &mut Self {
        match self.registrar.mode {
            TableRegistrarMode::Init(_) => {
                register_channel::<InsertUpdateMessage<TRow>>(self.registrar.expect_init());
            }
            TableRegistrarMode::Bind(_) => self
                .registrar
                .bind_insert_update::<TRow, TTable>(self.table),
        }
        self
    }
}

impl<'r, 't, TRow, TTable> TableBindingBuilder<'r, 't, TRow, TTable>
where
    TRow: Send + Sync + Clone + 'static,
    TTable: Table<Row = TRow> + NonEventTable,
{
    /// Forwards deletes as [`DeleteMessage`].
    pub fn delete(&mut self) -> &mut Self {
        match self.registrar.mode {
            TableRegistrarMode::Init(_) => {
                register_channel::<DeleteMessage<TRow>>(self.registrar.expect_init());
            }
            TableRegistrarMode::Bind(_) => self.registrar.bind_delete::<TRow, TTable>(self.table),
        }
        self
    }
}

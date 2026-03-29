//! Table registration for SpacetimeDB message forwarding.
//!
//! This module binds table callbacks and forwards table changes into Bevy messages.

use crate::{
    channel_bridge::{channel_sender, register_channel},
    message::{DeleteMessage, InsertMessage, InsertUpdateMessage, UpdateMessage},
};
use bevy_app::App;
use bevy_ecs::{message::Message, world::World};
use crossbeam_channel::Sender;
use spacetimedb_sdk::__codegen::DbContext;
use spacetimedb_sdk::{EventTable, Table, TableWithPrimaryKey};

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

    /// Registers a table with a primary key.
    ///
    /// Forwards table changes as:
    /// - [`InsertMessage`], [`DeleteMessage`], [`UpdateMessage`], and [`InsertUpdateMessage`]
    pub fn table<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow> + TableWithPrimaryKey<Row = TRow>,
    {
        match &mut self.mode {
            TableRegistrarMode::Init(app) => {
                let _ = register_channel::<InsertMessage<TRow>>(app);
                let _ = register_channel::<DeleteMessage<TRow>>(app);
                let _ = register_channel::<UpdateMessage<TRow>>(app);
                let _ = register_channel::<InsertUpdateMessage<TRow>>(app);
            }
            TableRegistrarMode::Bind(_) => {
                self.bind_insert(table);
                self.bind_delete(table);
                self.bind_update(table);
                self.bind_insert_update(table);
            }
        }
    }

    /// Registers a table without a primary key.
    ///
    /// Forwards inserts and deletes as [`InsertMessage`] and [`DeleteMessage`].
    pub fn table_without_pk<TRow, TTable>(&mut self, table: &TTable)
    where
        TRow: Send + Sync + Clone + 'static,
        TTable: Table<Row = TRow>,
    {
        match &mut self.mode {
            TableRegistrarMode::Init(app) => {
                let _ = register_channel::<InsertMessage<TRow>>(app);
                let _ = register_channel::<DeleteMessage<TRow>>(app);
            }
            TableRegistrarMode::Bind(_) => {
                self.bind_insert(table);
                self.bind_delete(table);
            }
        }
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
        match &mut self.mode {
            TableRegistrarMode::Init(app) => {
                let _ = register_channel::<InsertMessage<TRow>>(app);
            }
            TableRegistrarMode::Bind(_) => {
                self.bind_insert(table);
            }
        }
    }

    /// Returns the sender for the given message type and TableRegistrar mode.
    fn sender<T>(&mut self) -> Sender<T>
    where
        T: Message,
    {
        match &mut self.mode {
            TableRegistrarMode::Init(app) => register_channel::<T>(app),
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

//! Table registration and message forwarding for SpacetimeDB.
//!
//! Registers Bevy message channels and binds SDK table callbacks to
//! forward events as [`InsertMessage`](crate::message::InsertMessage),
//! [`UpdateMessage`](crate::message::UpdateMessage),
//! [`DeleteMessage`](crate::message::DeleteMessage), and
//! [`InsertUpdateMessage`](crate::message::InsertUpdateMessage).
use crate::{
    channel_bridge::{channel_sender, register_channel},
    message::{DeleteMessage, InsertMessage, InsertUpdateMessage, RowEvent, UpdateMessage},
};
use bevy_app::App;
use bevy_ecs::prelude::World;
use spacetimedb_sdk::{
    __codegen::{AbstractEventContext, DbContext, InModule, SpacetimeModule},
    EventTable, Table, TableWithPrimaryKey,
};
use std::marker::PhantomData;

/// Stored callback that performs one-time Bevy app registration for a table/view.
pub(crate) type TableRegistrationCallback = dyn Fn(&mut App) + Send + Sync;

/// Stored callback that binds SpacetimeDB table listeners for a concrete database view.
pub(crate) type TableBindCallback<C> =
    dyn for<'db> Fn(&World, &'db <C as DbContext>::DbView) + Send + Sync;

/// Binds callbacks for a table with a primary key.
///
/// Calling [`Self::bind`] attaches SpacetimeDB table callbacks and forwards
/// them as Bevy messages for insert, delete, update, and insert-or-update
/// changes.
pub struct TableBinder<'w, TRow> {
    world: &'w World,
    _marker: PhantomData<fn() -> TRow>,
}

impl<'w, TRow> TableBinder<'w, TRow> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self {
            world,
            _marker: PhantomData,
        }
    }

    /// Binds the default SpacetimeDB callbacks for `table` and forwards them as
    /// Bevy messages.
    pub fn bind<TTable>(self, table: TTable)
    where
        TRow: Send + Sync + Clone + InModule + 'static,
        RowEvent<TRow>: Send + Sync,
        TTable: Table<
                Row = TRow,
                EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
            > + TableWithPrimaryKey<Row = TRow>,
    {
        bind_insert::<TRow, TTable>(self.world, &table);
        bind_delete::<TRow, TTable>(self.world, &table);
        bind_update::<TRow, TTable>(self.world, &table);
        bind_insert_update::<TRow, TTable>(self.world, &table);
    }
}

/// Binds callbacks for a table without a primary key.
///
/// Calling [`Self::bind`] attaches SpacetimeDB table callbacks and forwards
/// insert and delete changes as Bevy messages.
pub struct TableWithoutPkBinder<'w, TRow> {
    world: &'w World,
    _marker: PhantomData<fn() -> TRow>,
}

impl<'w, TRow> TableWithoutPkBinder<'w, TRow> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self {
            world,
            _marker: PhantomData,
        }
    }

    /// Binds the default SpacetimeDB callbacks for `table` and forwards them as
    /// Bevy messages.
    pub fn bind<TTable>(self, table: TTable)
    where
        TRow: Send + Sync + Clone + InModule + 'static,
        RowEvent<TRow>: Send + Sync,
        TTable: Table<
                Row = TRow,
                EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
            >,
    {
        bind_insert::<TRow, TTable>(self.world, &table);
        bind_delete::<TRow, TTable>(self.world, &table);
    }
}

/// Binds callbacks for a view.
///
/// Calling [`Self::bind`] attaches SpacetimeDB table callbacks and forwards
/// insert and delete changes as Bevy messages.
pub struct ViewBinder<'w, TRow> {
    world: &'w World,
    _marker: PhantomData<fn() -> TRow>,
}

impl<'w, TRow> ViewBinder<'w, TRow> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self {
            world,
            _marker: PhantomData,
        }
    }

    /// Binds the default SpacetimeDB callbacks for `table` and forwards them as
    /// Bevy messages.
    pub fn bind<TTable>(self, table: TTable)
    where
        TRow: Send + Sync + Clone + InModule + 'static,
        RowEvent<TRow>: Send + Sync,
        TTable: Table<
                Row = TRow,
                EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
            >,
    {
        bind_insert::<TRow, TTable>(self.world, &table);
        bind_delete::<TRow, TTable>(self.world, &table);
    }
}

/// Binds callbacks for an event table.
///
/// Calling [`Self::bind`] attaches SpacetimeDB table callbacks and forwards
/// insert changes as Bevy messages.
pub struct EventTableBinder<'w, TRow> {
    world: &'w World,
    _marker: PhantomData<fn() -> TRow>,
}

impl<'w, TRow> EventTableBinder<'w, TRow> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self {
            world,
            _marker: PhantomData,
        }
    }

    /// Binds the default SpacetimeDB callbacks for `table` and forwards them as
    /// Bevy messages.
    pub fn bind<TTable>(self, table: TTable)
    where
        TRow: Send + Sync + Clone + InModule + 'static,
        RowEvent<TRow>: Send + Sync,
        TTable: Table<
                Row = TRow,
                EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
            > + EventTable,
    {
        bind_insert::<TRow, TTable>(self.world, &table);
    }
}

/// Registers Bevy message channels for a table with a primary key.
pub(crate) fn register_table<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
{
    register_channel::<InsertMessage<TRow>>(app);
    register_channel::<DeleteMessage<TRow>>(app);
    register_channel::<UpdateMessage<TRow>>(app);
    register_channel::<InsertUpdateMessage<TRow>>(app);
}

/// Registers Bevy message channels for a table without a primary key.
pub(crate) fn register_table_without_pk<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
{
    register_channel::<InsertMessage<TRow>>(app);
    register_channel::<DeleteMessage<TRow>>(app);
}

/// Registers Bevy message channels for a view.
pub(crate) fn register_view<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
{
    register_table_without_pk::<TRow>(app);
}

/// Registers Bevy message channels for an event table.
pub(crate) fn register_event_table<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
{
    register_channel::<InsertMessage<TRow>>(app);
}

fn bind_insert<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
    TTable: Table<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        >,
    TTable::EventContext: AbstractEventContext<Event = RowEvent<TRow>>,
{
    let sender = channel_sender::<InsertMessage<TRow>>(world);
    table.on_insert(move |ctx, row| {
        let _ = sender.send(InsertMessage {
            event: ctx.event().clone(),
            row: row.clone(),
        });
    });
}

fn bind_delete<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
    TTable: Table<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        >,
    TTable::EventContext: AbstractEventContext<Event = RowEvent<TRow>>,
{
    let sender = channel_sender::<DeleteMessage<TRow>>(world);
    table.on_delete(move |ctx, row| {
        let _ = sender.send(DeleteMessage {
            event: ctx.event().clone(),
            row: row.clone(),
        });
    });
}

fn bind_update<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
    TTable: Table<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + TableWithPrimaryKey<Row = TRow>,
    TTable::EventContext: AbstractEventContext<Event = RowEvent<TRow>>,
{
    let sender = channel_sender::<UpdateMessage<TRow>>(world);
    table.on_update(move |ctx, old, new| {
        let _ = sender.send(UpdateMessage {
            event: ctx.event().clone(),
            old: old.clone(),
            new: new.clone(),
        });
    });
}

fn bind_insert_update<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
    TTable: Table<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + TableWithPrimaryKey<Row = TRow>,
    TTable::EventContext: AbstractEventContext<Event = RowEvent<TRow>>,
{
    let sender_insert = channel_sender::<InsertUpdateMessage<TRow>>(world);
    table.on_insert(move |ctx, row| {
        let _ = sender_insert.send(InsertUpdateMessage {
            event: ctx.event().clone(),
            old: None,
            new: row.clone(),
        });
    });

    let sender_update = channel_sender::<InsertUpdateMessage<TRow>>(world);
    table.on_update(move |ctx, old, new| {
        let _ = sender_update.send(InsertUpdateMessage {
            event: ctx.event().clone(),
            old: Some(old.clone()),
            new: new.clone(),
        });
    });
}

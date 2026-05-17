use crate::{
    channel_bridge::channel_sender,
    message::{DeleteMessage, InsertMessage, InsertUpdateMessage, RowEvent, UpdateMessage},
};
use bevy_ecs::prelude::World;
use spacetimedb_sdk::{
    __codegen::{AbstractEventContext, InModule, SpacetimeModule},
    EventTable, Table, TableWithPrimaryKey,
};
use std::marker::PhantomData;

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
        TTable: EventTable<
                Row = TRow,
                EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
            >,
    {
        // bind_insert::<TRow, TTable>(self.world, &table);
        // Temporarily inline this until spacetime is able to distinguish insert capabilities separately from event tables.
        let sender = channel_sender::<InsertMessage<TRow>>(self.world);
        table.on_insert(move |ctx, row| {
            let _ = sender.send(InsertMessage {
                event: ctx.event().clone(),
                row: row.clone(),
            });
        });
    }
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

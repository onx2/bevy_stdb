use crate::{
    channel_bridge::channel_sender,
    message::{DeleteMessage, InsertMessage, InsertUpdateMessage, RowEvent, UpdateMessage},
};
use bevy_ecs::prelude::World;
use spacetimedb_sdk::__codegen::{
    AbstractEventContext, InModule, SpacetimeModule, TableLike, WithDelete, WithInsert, WithUpdate,
};

pub(crate) fn bind_table<TRow, TTable>(world: &World, table: TTable)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
    TTable: TableLike<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithInsert
        + WithDelete
        + WithUpdate,
{
    bind_insert::<TRow, TTable>(world, &table);
    bind_delete::<TRow, TTable>(world, &table);
    bind_update::<TRow, TTable>(world, &table);
    bind_insert_update::<TRow, TTable>(world, &table);
}

pub(crate) fn bind_table_without_pk<TRow, TTable>(world: &World, table: TTable)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
    TTable: TableLike<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithInsert
        + WithDelete,
{
    bind_insert::<TRow, TTable>(world, &table);
    bind_delete::<TRow, TTable>(world, &table);
}

pub(crate) fn bind_insert<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
    TTable: TableLike<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithInsert,
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
    TTable: TableLike<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithDelete,
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
    TTable: TableLike<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithUpdate,
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
    TTable: TableLike<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithInsert
        + WithUpdate,
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

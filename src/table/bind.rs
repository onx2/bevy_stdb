use crate::{
    channel_bridge::channel_sender,
    message::{RowEvent, TableChange},
};
use bevy_ecs::prelude::World;
use spacetimedb_sdk::__codegen::{
    AbstractEventContext, InModule, SpacetimeModule, TableLike, WithDelete, WithInsert, WithUpdate,
};
use std::sync::Arc;

/// Forwards `on_insert` into the shared [`TableChange`] channel for `TRow`.
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
    let sender = channel_sender::<TableChange<TRow>>(world);
    table.on_insert(move |ctx, row| {
        let _ = sender.send(TableChange::Insert {
            event: Arc::new(ctx.event().clone()),
            row: row.clone(),
        });
    });
}

/// Forwards `on_delete` into the shared [`TableChange`] channel for `TRow`.
pub(crate) fn bind_delete<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
    TTable: TableLike<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithDelete,
    TTable::EventContext: AbstractEventContext<Event = RowEvent<TRow>>,
{
    let sender = channel_sender::<TableChange<TRow>>(world);
    table.on_delete(move |ctx, row| {
        let _ = sender.send(TableChange::Delete {
            event: Arc::new(ctx.event().clone()),
            row: row.clone(),
        });
    });
}

/// Forwards `on_update` into the shared [`TableChange`] channel for `TRow`.
pub(crate) fn bind_update<TRow, TTable>(world: &World, table: &TTable)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
    TTable: TableLike<
            Row = TRow,
            EventContext = <<TRow as InModule>::Module as SpacetimeModule>::EventContext,
        > + WithUpdate,
    TTable::EventContext: AbstractEventContext<Event = RowEvent<TRow>>,
{
    let sender = channel_sender::<TableChange<TRow>>(world);
    table.on_update(move |ctx, old, new| {
        let _ = sender.send(TableChange::Update {
            event: Arc::new(ctx.event().clone()),
            old: old.clone(),
            new: new.clone(),
        });
    });
}

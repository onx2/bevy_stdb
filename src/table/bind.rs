use super::policy::carries_event;
use crate::{
    channel_bridge::channel_sender,
    message::{RowEvent, SharedRowEvent, TableChange},
};
use bevy_ecs::prelude::World;
use spacetimedb_sdk::__codegen::{
    AbstractEventContext, InModule, SpacetimeModule, TableLike, WithDelete, WithInsert, WithUpdate,
};

/// Forwards `on_insert` into the shared [`TableChange`] channel for `TRow`.
///
/// The event is cloned only when `TRow` carries one; see [`carries_event`].
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
    let with_event = carries_event::<TRow>(world);
    table.on_insert(move |ctx, row| {
        let _ = sender.send(TableChange::Insert {
            event: with_event.then(|| SharedRowEvent::<TRow>::new(ctx.event().clone())),
            row: row.clone(),
        });
    });
}

/// Forwards `on_delete` into the shared [`TableChange`] channel for `TRow`.
///
/// The event is cloned only when `TRow` carries one; see [`carries_event`].
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
    let with_event = carries_event::<TRow>(world);
    table.on_delete(move |ctx, row| {
        let _ = sender.send(TableChange::Delete {
            event: with_event.then(|| SharedRowEvent::<TRow>::new(ctx.event().clone())),
            row: row.clone(),
        });
    });
}

/// Forwards `on_update` into the shared [`TableChange`] channel for `TRow`.
///
/// The event is cloned only when `TRow` carries one; see [`carries_event`].
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
    let with_event = carries_event::<TRow>(world);
    table.on_update(move |ctx, old, new| {
        let _ = sender.send(TableChange::Update {
            event: with_event.then(|| SharedRowEvent::<TRow>::new(ctx.event().clone())),
            old: old.clone(),
            new: new.clone(),
        });
    });
}

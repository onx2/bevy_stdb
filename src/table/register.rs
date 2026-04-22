use crate::{
    channel_bridge::register_channel,
    message::{DeleteMessage, InsertMessage, InsertUpdateMessage, RowEvent, UpdateMessage},
};
use bevy_app::App;
use spacetimedb_sdk::__codegen::InModule;

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

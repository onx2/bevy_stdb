use crate::{
    channel_bridge::register_channel,
    message::{DeleteMessage, InsertMessage, InsertUpdateMessage, RowEvent, UpdateMessage},
};
use bevy_app::App;
use spacetimedb_sdk::__codegen::InModule;

pub(crate) fn register_insert<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
{
    register_channel::<InsertMessage<TRow>>(app);
}

pub(crate) fn register_delete<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
{
    register_channel::<DeleteMessage<TRow>>(app);
}

pub(crate) fn register_update<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
{
    register_channel::<UpdateMessage<TRow>>(app);
}

pub(crate) fn register_insert_update<TRow>(app: &mut App)
where
    TRow: Send + Sync + Clone + InModule + 'static,
    RowEvent<TRow>: Send + Sync,
{
    register_channel::<InsertUpdateMessage<TRow>>(app);
}

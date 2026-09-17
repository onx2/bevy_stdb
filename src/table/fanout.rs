//! Derivation of the typed row messages from the shared [`TableChange`] stream.
//!
//! SDK row callbacks feed one [`TableChange`] channel per row type. The systems here run in
//! [`StdbSet::Flush`] after the channel drain and project that stream into the typed messages a
//! capability was bound for, so a row callback clones its event once no matter how many streams
//! observe it.
use crate::{
    channel_bridge::drain_channels,
    message::{
        DeleteMessage, InsertMessage, InsertUpdateMessage, RowEvent, TableChange, UpdateMessage,
    },
    set::StdbSet,
};
use bevy_app::{App, PreUpdate};
use bevy_ecs::{
    prelude::{MessageReader, MessageWriter},
    schedule::IntoScheduleConfigs,
};
use spacetimedb_sdk::__codegen::InModule;
use std::sync::Arc;

/// Adds the derived message `TMessage` and the system that projects it from [`TableChange`].
macro_rules! fanout {
    ($register:ident, $system:ident, $message:ident<$row:ident>, |$change:ident| $project:expr) => {
        pub(crate) fn $register<$row>(app: &mut App)
        where
            $row: Send + Sync + Clone + InModule + 'static,
            RowEvent<$row>: Send + Sync,
        {
            app.add_message::<$message<$row>>();
            app.add_systems(
                PreUpdate,
                $system::<$row>.in_set(StdbSet::Flush).after(drain_channels),
            );
        }

        fn $system<$row>(
            mut changes: MessageReader<TableChange<$row>>,
            mut writer: MessageWriter<$message<$row>>,
        ) where
            $row: Send + Sync + Clone + InModule + 'static,
            RowEvent<$row>: Send + Sync,
        {
            for $change in changes.read() {
                if let Some(message) = $project {
                    writer.write(message);
                }
            }
        }
    };
}

fanout!(
    register_insert_fanout,
    fan_out_insert,
    InsertMessage<TRow>,
    |change| match change {
        TableChange::Insert { event, row } => Some(InsertMessage {
            event: Arc::clone(event),
            row: row.clone(),
        }),
        _ => None,
    }
);

fanout!(
    register_delete_fanout,
    fan_out_delete,
    DeleteMessage<TRow>,
    |change| match change {
        TableChange::Delete { event, row } => Some(DeleteMessage {
            event: Arc::clone(event),
            row: row.clone(),
        }),
        _ => None,
    }
);

fanout!(
    register_update_fanout,
    fan_out_update,
    UpdateMessage<TRow>,
    |change| match change {
        TableChange::Update { event, old, new } => Some(UpdateMessage {
            event: Arc::clone(event),
            old: old.clone(),
            new: new.clone(),
        }),
        _ => None,
    }
);

fanout!(
    register_insert_update_fanout,
    fan_out_insert_update,
    InsertUpdateMessage<TRow>,
    |change| match change {
        TableChange::Insert { event, row } => Some(InsertUpdateMessage {
            event: Arc::clone(event),
            old: None,
            new: row.clone(),
        }),
        TableChange::Update { event, old, new } => Some(InsertUpdateMessage {
            event: Arc::clone(event),
            old: Some(old.clone()),
            new: new.clone(),
        }),
        TableChange::Delete { .. } => None,
    }
);

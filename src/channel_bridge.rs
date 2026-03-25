//! Channel-backed message delivery for Bevy.
//!
//! This module registers per-type channels and forwards queued values into `Messages<T>`.

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::{
    message::{Message, Messages},
    resource::Resource,
    world::World,
};
use std::{
    any::{Any, TypeId},
    sync::{
        Mutex,
        mpsc::{Sender, channel},
    },
};

/// A type-erased function that drains a channel into `Messages<T>`.
type DrainFn = Box<dyn Fn(&mut World) + Send + Sync>;

/// A type-erased function that clones a registered `Sender<T>`.
type CloneSenderFn = Box<dyn Fn() -> Box<dyn Any + Send> + Send + Sync>;

/// Stores the registered message channels.
struct ChannelEntry {
    type_id: TypeId,
    drain: DrainFn,
    clone_sender: CloneSenderFn,
}

#[derive(Resource, Default)]
pub(crate) struct ChannelRegistry {
    channels: Vec<ChannelEntry>,
}

pub(crate) struct ChannelBridgePlugin;
impl Plugin for ChannelBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChannelRegistry>();

        // Drain all registered channels once per frame.
        app.add_systems(PreUpdate, |world: &mut World| {
            let Some(reg) = world.remove_resource::<ChannelRegistry>() else {
                return;
            };
            reg.channels.iter().for_each(|e| (e.drain)(world));
            world.insert_resource(reg);
        });
    }
}

/// Registers a channel for message type `T`.
///
/// Returns an existing sender if the channel has already been registered.
///
/// # Panics
///
/// Panics if [`ChannelRegistry`] has not been initialized.
pub(crate) fn register_channel<T: Message>(app: &mut App) -> Sender<T> {
    if let Some(sender) = channel_sender::<T>(app.world()) {
        return sender;
    }

    let (tx, rx) = channel::<T>();
    let tx_for_lookup = tx.clone();
    let rx = Mutex::new(rx);

    app.add_message::<T>();

    app.world_mut()
        .resource_mut::<ChannelRegistry>()
        .channels
        .push(ChannelEntry {
            type_id: TypeId::of::<T>(),
            drain: Box::new(move |world: &mut World| {
                let msgs: Vec<T> = {
                    let rx = rx.lock().unwrap_or_else(|e| e.into_inner());
                    rx.try_iter().collect()
                };

                if msgs.is_empty() {
                    return;
                }

                let Some(mut messages) = world.get_resource_mut::<Messages<T>>() else {
                    return;
                };

                messages.write_batch(msgs);
            }),
            clone_sender: Box::new(move || Box::new(tx_for_lookup.clone())),
        });

    tx
}

/// Returns the registered `Sender<T>`, if one exists.
pub(crate) fn channel_sender<T: Message>(world: &World) -> Option<Sender<T>> {
    let registry = world.get_resource::<ChannelRegistry>()?;

    let entry = registry
        .channels
        .iter()
        .find(|entry| entry.type_id == TypeId::of::<T>())?;

    let boxed = (entry.clone_sender)();
    boxed.downcast::<Sender<T>>().ok().map(|sender| *sender)
}

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
/// # Panics
///
/// Panics if [`ChannelRegistry`] has not been initialized or if the
/// channel for `T` has already been registered.
pub(crate) fn register_channel<T: Message>(app: &mut App) {
    assert!(
        !app.world()
            .resource::<ChannelRegistry>()
            .channels
            .iter()
            .any(|entry| entry.type_id == TypeId::of::<T>()),
        "attempted to register channel for message type `{}` more than once",
        std::any::type_name::<T>(),
    );

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
}

/// Returns the registered `Sender<T>`.
///
/// # Panics
///
/// Panics if [`ChannelRegistry`] has not been initialized, if the
/// channel for `T` has not been registered, or if the stored sender
/// has an unexpected concrete type.
pub(crate) fn channel_sender<T: Message>(world: &World) -> Sender<T> {
    let registry = world
        .get_resource::<ChannelRegistry>()
        .expect("channel registry should be initialized before accessing channel senders");

    let entry = registry
        .channels
        .iter()
        .find(|entry| entry.type_id == TypeId::of::<T>())
        .unwrap_or_else(|| {
            panic!(
                "channel for message type `{}` should be registered before accessing its sender",
                std::any::type_name::<T>(),
            )
        });

    let boxed = (entry.clone_sender)();
    boxed
        .downcast::<Sender<T>>()
        .unwrap_or_else(|_| {
            panic!(
                "stored sender for message type `{}` had an unexpected concrete type",
                std::any::type_name::<T>(),
            )
        })
        .as_ref()
        .clone()
}

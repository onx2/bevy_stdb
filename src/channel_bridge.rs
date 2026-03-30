//! Channel-backed message delivery for Bevy.
//!
//! This module registers per-type channels and forwards queued values into `Messages<T>`.

use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{Message, Messages, Mut, Resource, World};
use crossbeam_channel::{Sender, unbounded};
use std::any::{Any, TypeId};

/// Stores the registered message channels.
struct ChannelEntry {
    /// The registered message type.
    type_id: TypeId,
    /// A type-erased function that drains a channel into `Messages<T>`.
    drain: Box<dyn Fn(&mut World) + Send + Sync>,
    /// The sender for this message type.
    sender: Box<dyn Any + Send + Sync>,
}

#[derive(Resource, Default)]
pub(crate) struct ChannelRegistry {
    channels: Vec<ChannelEntry>,
}

pub(crate) struct ChannelBridgePlugin;
impl Plugin for ChannelBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChannelRegistry>();
        app.add_systems(PreUpdate, drain_channels);
    }
}

/// Drain all registered channels once per frame.
fn drain_channels(world: &mut World) {
    world.resource_scope(|world, registry: Mut<ChannelRegistry>| {
        for entry in &registry.channels {
            (entry.drain)(world);
        }
    });
}

/// Registers a channel for message type `T`.
///
/// # Panics
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

    let (tx, rx) = unbounded::<T>();
    app.add_message::<T>();

    app.world_mut()
        .resource_mut::<ChannelRegistry>()
        .channels
        .push(ChannelEntry {
            type_id: TypeId::of::<T>(),
            drain: Box::new(move |world: &mut World| {
                let mut messages = world.resource_mut::<Messages<T>>();
                messages.write_batch(rx.try_iter());
            }),
            sender: Box::new(tx),
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
    let registry = world.resource::<ChannelRegistry>();

    let entry = registry
        .channels
        .iter()
        .find(|entry| entry.type_id == TypeId::of::<T>())
        .unwrap_or_else(|| panic!("unregistered channel for `{}`", std::any::type_name::<T>()));

    entry
        .sender
        .as_ref()
        .downcast_ref::<Sender<T>>()
        .unwrap_or_else(|| panic!("unexpected type for sender`{}`", std::any::type_name::<T>(),))
        .clone()
}

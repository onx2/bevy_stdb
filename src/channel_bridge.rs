//! Channel-backed message delivery for Bevy.
//!
//! Registers per-type channels and forwards messages from those
//! channels into Bevy [`Messages<T>`](bevy_ecs::prelude::Messages), such as SpacetimeDB table events
//! or connection lifecycle messages.
use crate::message::StdbCustomMessage;
use crate::set::StdbSet;
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Message, Messages, Mut, Resource, World};
use crossbeam_channel::{Sender, unbounded};
use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;

/// Stores the registered message channels.
struct ChannelEntry {
    /// The registered message type.
    type_id: TypeId,
    /// A type-erased function that drains a channel into `Messages<T>`.
    drain: Box<dyn Fn(&mut World) + Send + Sync>,
    /// The sender for this message type.
    sender: Box<dyn Any + Send + Sync>,
}

/// Registry of per-type message channels.
#[derive(Resource, Default)]
struct ChannelRegistry {
    channels: Vec<ChannelEntry>,
}

/// Initializes the channel registry and installs the per-frame drain system.
pub(crate) struct ChannelBridgePlugin;
impl Plugin for ChannelBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChannelRegistry>();
        app.init_resource::<StdbChannels>();
        app.add_systems(PreUpdate, drain_channels.in_set(StdbSet::Flush));
    }
}

/// Drains all registered channels once per frame.
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
        type_name::<T>(),
    );

    let (tx, rx) = unbounded::<T>();
    app.add_message::<T>();

    app.world_mut()
        .resource_mut::<ChannelRegistry>()
        .channels
        .push(ChannelEntry {
            type_id: TypeId::of::<T>(),
            drain: Box::new(move |world: &mut World| {
                world
                    .resource_mut::<Messages<T>>()
                    .write_batch(rx.try_iter());
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
        .unwrap_or_else(|| panic!("unregistered channel for `{}`", type_name::<T>()));

    entry
        .sender
        .as_ref()
        .downcast_ref::<Sender<T>>()
        .unwrap_or_else(|| panic!("unexpected type for sender `{}`", type_name::<T>(),))
        .clone()
}

/// Public registry of consumer-defined bridged channels.
///
/// Holds the sender for every payload type registered with
/// [`StdbPlugin::add_custom_message`](crate::prelude::StdbPlugin::add_custom_message).
/// Pull a sender with [`Self::sender`] and move it into a callback or thread
/// running off the Bevy schedule (a SpacetimeDB reducer/procedure `_then`
/// handler, an HTTP response handler, a background task, ...) to forward a value
/// into Bevy. Read it back with
/// [`ReadStdbCustomMessage<T>`](crate::prelude::ReadStdbCustomMessage).
///
/// Only the cloned sender is moved into the callback; the resource itself stays
/// in the [`World`].
#[derive(Resource, Default)]
pub struct StdbChannels {
    /// Type-erased `Sender<StdbCustomMessage<T>>` keyed by `TypeId::of::<StdbCustomMessage<T>>()`.
    senders: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl StdbChannels {
    /// Returns a clone of the sender for the channel carrying payload `T`.
    ///
    /// Move the returned sender into a callback and forward with
    /// `tx.send(StdbCustomMessage(value))`. The resource itself is never
    /// captured.
    ///
    /// # Panics
    ///
    /// Panics if no channel for `T` was registered via
    /// [`StdbPlugin::add_custom_message`](crate::prelude::StdbPlugin::add_custom_message).
    pub fn sender<T: Send + Sync + 'static>(&self) -> Sender<StdbCustomMessage<T>> {
        self.senders
            .get(&TypeId::of::<StdbCustomMessage<T>>())
            .and_then(|tx| tx.downcast_ref::<Sender<StdbCustomMessage<T>>>())
            .unwrap_or_else(|| {
                panic!(
                    "no channel registered for `{0}`; call `StdbPlugin::add_custom_message::<{0}>()`",
                    type_name::<T>(),
                )
            })
            .clone()
    }
}

/// Registers a bridged channel for `T` and stores its sender in [`StdbChannels`].
///
/// # Panics
///
/// Panics if the channel for `T` has already been registered, or if
/// [`StdbChannels`] has not been initialized.
pub(crate) fn register_bridged_channel<T: Message>(app: &mut App) {
    register_channel::<T>(app);
    let sender = channel_sender::<T>(app.world());
    app.world_mut()
        .resource_mut::<StdbChannels>()
        .senders
        .insert(TypeId::of::<T>(), Box::new(sender));
}

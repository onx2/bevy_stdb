//! Channel-backed message delivery for Bevy.
//!
//! Registers per-type channels and forwards messages from those
//! channels into Bevy [`Messages<T>`](bevy_ecs::prelude::Messages), such as SpacetimeDB table events
//! or connection lifecycle messages.
use crate::set::StdbSet;
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Message, Messages, Mut, Resource, World};
use crossbeam_channel::{Sender, unbounded};
use std::any::{Any, TypeId, type_name};
use std::sync::Arc;

pub type ChannelRegistrationCallback = dyn Fn(&mut App) + Send + Sync;

/// Stores one registered message channel.
struct ChannelEntry {
    /// The registered message type.
    type_id: TypeId,
    /// A type-erased function that drains a channel into `Messages<T>`.
    drain: Box<dyn Fn(&mut World) + Send + Sync>,
    /// The sender for this message type.
    sender: Box<dyn Any + Send + Sync>,
}

/// Registry of per-type message channels that bridge crossbeam senders into
/// Bevy [`Messages<T>`](bevy_ecs::prelude::Messages).
///
/// Internal channels (connection lifecycle, table events, subscriptions) and
/// consumer channels registered with
/// [`StdbPlugin::add_channel_message`](crate::prelude::StdbPlugin::add_channel_message)
/// all live here. Consumers obtain a sender for their message type with [`Self::sender`]
/// and move it into a callback or task running off the Bevy schedule (a
/// SpacetimeDB reducer/procedure `_then` handler, an HTTP response handler, an
/// `IoTaskPool` task, ...) to forward a value into Bevy; read it back with
/// `MessageReader<T>`.
#[derive(Resource, Default)]
pub struct StdbChannels {
    channels: Vec<ChannelEntry>,
}

impl StdbChannels {
    /// Returns a clone of the registered [`Sender<T>`] for message type `T`.
    ///
    /// Move the returned sender into a callback or task and forward with
    /// `tx.send(message)`. Only the cloned sender is captured; the resource
    /// itself stays in the [`World`].
    ///
    /// # Panics
    ///
    /// Panics if no channel for `T` was registered via
    /// [`StdbPlugin::add_channel_message`](crate::prelude::StdbPlugin::add_channel_message).
    pub fn sender<T: Message>(&self) -> Sender<T> {
        self.channels
            .iter()
            .find(|entry| entry.type_id == TypeId::of::<T>())
            .and_then(|entry| entry.sender.downcast_ref::<Sender<T>>())
            .unwrap_or_else(|| panic!("unregistered channel for `{}`", type_name::<T>()))
            .clone()
    }
}

/// Initializes the channel registry, installs the per-frame drain system, and
/// runs the deferred consumer channel registrations.
pub(crate) struct ChannelBridgePlugin {
    pub(crate) channel_registrations: Vec<Arc<dyn Fn(&mut App) + Send + Sync>>,
}
impl Plugin for ChannelBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StdbChannels>();
        app.add_systems(PreUpdate, drain_channels.in_set(StdbSet::Flush));

        for register in &self.channel_registrations {
            register(app);
        }
    }
}

/// Drains all registered channels once per frame.
fn drain_channels(world: &mut World) {
    world.resource_scope(|world, registry: Mut<StdbChannels>| {
        for entry in &registry.channels {
            (entry.drain)(world);
        }
    });
}

/// Registers a channel for message type `T`.
///
/// # Panics
///
/// Panics if [`StdbChannels`] has not been initialized or if the channel for
/// `T` has already been registered.
pub(crate) fn register_channel<T: Message>(app: &mut App) {
    assert!(
        !app.world()
            .resource::<StdbChannels>()
            .channels
            .iter()
            .any(|entry| entry.type_id == TypeId::of::<T>()),
        "attempted to register channel for message type `{}` more than once",
        type_name::<T>(),
    );

    let (tx, rx) = unbounded::<T>();
    app.add_message::<T>();

    app.world_mut()
        .resource_mut::<StdbChannels>()
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

/// Returns the registered `Sender<T>` from the world.
///
/// # Panics
///
/// Panics if [`StdbChannels`] has not been initialized, or if the channel for
/// `T` has not been registered.
pub(crate) fn channel_sender<T: Message>(world: &World) -> Sender<T> {
    world.resource::<StdbChannels>().sender::<T>()
}

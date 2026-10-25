//! System sets for ordering `bevy_stdb` systems.
use bevy_ecs::prelude::SystemSet;

/// System sets for `bevy_stdb` systems in [`PreUpdate`](bevy_app::PreUpdate).
///
/// # Example
///
/// ```ignore
/// app.add_systems(
///     PreUpdate,
///     my_system.after(StdbSet::Flush),
/// );
/// ```
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum StdbSet {
    /// Advances a connection configured with
    /// [`StdbPlugin::with_frame_driver`](crate::prelude::StdbPlugin::with_frame_driver), which
    /// runs its SDK callbacks. Ahead of [`Self::Flush`] so what they deliver is read this frame.
    Drive,
    /// Drains SDK message channels into Bevy [`Messages`](bevy_ecs::prelude::Messages).
    Flush,
    /// Synchronizes connection state from lifecycle messages.
    StateSync,
    /// Manages connection lifecycle: building, activating, and reconnect.
    Connection,
    /// Applies queued subscriptions to the active connection.
    Subscriptions,
}

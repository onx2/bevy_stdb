//! System sets for ordering `bevy_stdb` systems.
use bevy_ecs::prelude::SystemSet;

/// System sets for `bevy_stdb` systems in [`PreUpdate`](bevy_app::PreUpdate).
///
/// Sets run in declaration order:
/// [`Flush`](Self::Flush) → [`StateSync`](Self::StateSync) →
/// [`Connection`](Self::Connection) → [`Subscriptions`](Self::Subscriptions).
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
    /// Drains SDK message channels into Bevy [`Messages`](bevy_ecs::prelude::Messages).
    Flush,
    /// Synchronizes [`StdbConnectionState`](crate::connection::StdbConnectionState) from lifecycle messages.
    StateSync,
    /// Manages connection lifecycle: building, finalizing, driving, and reconnect.
    Connection,
    /// Applies queued subscriptions to the active connection.
    Subscriptions,
}

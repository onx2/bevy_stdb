//! Reconnect policy and runtime state for SpacetimeDB connections.
//!
<<<<<<< HEAD
//! Manages reconnect timing and backoff. When a disconnect with an error or a
//! disconnect error message is received, a reconnect timer is scheduled. When
//! the timer fires, a connection task is spawned directly.

use super::{PendingConnection, StdbConnection, StdbConnectionConfig};
use crate::{
    alias::{ReadStdbConnectedMessage, ReadStdbDisconnectedMessage},
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{Commands, IntoScheduleConfigs, Res, ResMut, Resource};
=======
//! Manages reconnect timing and backoff. When a disconnect is received the
//! reconnect cycle activates. Each tick the timer is advanced and, once it
//! fires and no [`PendingConnection`] is in-flight, a new connection task is
//! spawned. A successful connect resets the cycle.

use super::{PendingConnection, StdbConnection, StdbConnectionConfig};
use crate::{
    alias::{ReadStdbConnectErrorMessage, ReadStdbConnectedMessage, ReadStdbDisconnectedMessage},
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{
    Commands, IntoScheduleConfigs, Res, ResMut, Resource, not, resource_exists,
};
>>>>>>> origin/main
use bevy_tasks::IoTaskPool;
use bevy_time::{Time, Timer, TimerMode};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::{marker::PhantomData, ops::Deref, time::Duration};

/// Reconnect options for a SpacetimeDB connection.
#[derive(Clone, Debug)]
pub struct StdbReconnectOptions {
    /// Delay before the first reconnect attempt after a disconnect.
    pub initial_delay: Duration,
    /// Maximum number of reconnect attempts before giving up.
    ///
    /// `0` retries indefinitely.
    pub max_attempts: u32,
<<<<<<< HEAD
    /// Multiplier applied after each failed reconnect attempt.
=======
    /// Multiplier applied to the current delay after each failed attempt.
>>>>>>> origin/main
    ///
    /// Values below `1.0` are clamped to `1.0` to prevent the delay from
    /// shrinking between attempts.
    pub backoff_factor: f32,
    /// Maximum delay between reconnect attempts.
    pub max_delay: Duration,
}

impl Default for StdbReconnectOptions {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_attempts: 0,
            backoff_factor: 1.5,
            max_delay: Duration::from_secs(15),
        }
    }
}

<<<<<<< HEAD
/// Runtime reconnect configuration.
=======
/// Runtime reconnect configuration resource.
>>>>>>> origin/main
#[derive(Resource, Clone)]
struct ReconnectConfig(pub StdbReconnectOptions);

impl Deref for ReconnectConfig {
    type Target = StdbReconnectOptions;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

<<<<<<< HEAD
/// Runtime state for reconnect attempts.
#[derive(Resource, Default)]
struct ReconnectBackoff {
    /// Whether a reconnect cycle is currently active.
    active: bool,
    /// Number of reconnect attempts made in the current cycle.
    attempts: u32,
    /// Current delay between reconnect attempts.
    current_delay: Duration,
    /// Timer for the next reconnect attempt.
=======
/// Runtime state for the active reconnect cycle.
///
/// The presence of `timer` signals that a reconnect cycle is active.
#[derive(Resource, Default)]
struct ReconnectBackoff {
    /// Number of reconnect attempts made in the current cycle.
    attempts: u32,
    /// Delay that will be used for the next reconnect attempt.
    current_delay: Duration,
    /// Countdown timer for the next reconnect attempt.
>>>>>>> origin/main
    timer: Option<Timer>,
}

/// Internal plugin for reconnect timing and backoff.
pub(crate) struct ReconnectPlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    reconnect_options: StdbReconnectOptions,
    _marker: PhantomData<(C, M)>,
}

impl<C, M> ReconnectPlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    /// Creates a new [`ReconnectPlugin`] with the given options.
    pub(crate) fn new(reconnect_options: StdbReconnectOptions) -> Self {
        Self {
            reconnect_options,
            _marker: PhantomData,
        }
    }
}

impl<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
> Plugin for ReconnectPlugin<C, M>
{
    fn build(&self, app: &mut App) {
        app.insert_resource(ReconnectConfig(self.reconnect_options.clone()));
        app.init_resource::<ReconnectBackoff>();

        app.add_systems(
            PreUpdate,
<<<<<<< HEAD
            (
                reset_reconnect_state,
                update_reconnect_backoff::<C>,
                tick_reconnect_timer::<C, M>,
            )
                .chain()
=======
            (on_connect, arm_reconnect_timer).in_set(StdbSet::Connection),
        );

        app.add_systems(
            PreUpdate,
            tick_reconnect_timer::<C, M>
                .run_if(not(resource_exists::<StdbConnection<C>>))
>>>>>>> origin/main
                .in_set(StdbSet::Connection),
        );
    }
}

<<<<<<< HEAD
/// Starts or advances the reconnect cycle based on connection lifecycle messages.
fn update_reconnect_backoff<C: DbContext + Send + Sync + 'static>(
    reconnect_config: Res<ReconnectConfig>,
    mut reconnect: ResMut<ReconnectBackoff>,
    mut connected_msgs: ReadStdbConnectedMessage,
    mut disconnected_msgs: ReadStdbDisconnectedMessage,
    conn: Option<Res<StdbConnection<C>>>,
) {
    if connected_msgs.read().next().is_some() {
        return;
    }

    let saw_disconnect_error = disconnected_msgs.read().any(|msg| msg.err.is_some());

    if !saw_disconnect_error {
        return;
    }

    let active_conn = conn.as_ref().map(|conn| conn.is_active()).unwrap_or(false);
    if active_conn {
        return;
    }

    if reconnect.active {
        reconnect.attempts += 1;

        if reconnect_config.max_attempts > 0 && reconnect.attempts >= reconnect_config.max_attempts
        {
            reconnect.active = false;
            reconnect.timer = None;
            return;
        }

        let next_delay = reconnect
            .current_delay
            .mul_f32(reconnect_config.backoff_factor.max(1.0));
        reconnect.current_delay = next_delay.min(reconnect_config.max_delay);
    } else {
        reconnect.active = true;
        reconnect.attempts = 0;
        reconnect.current_delay = reconnect_config.initial_delay;
    }

    reconnect.timer = Some(Timer::new(reconnect.current_delay, TimerMode::Once));
}

/// Resets reconnect state when a connection is successfully established.
fn reset_reconnect_state(
    mut reconnect: ResMut<ReconnectBackoff>,
    mut connected_msgs: ReadStdbConnectedMessage,
) {
    if connected_msgs.read().next().is_none() {
        return;
    }

    reconnect.active = false;
    reconnect.attempts = 0;
    reconnect.current_delay = Duration::ZERO;
    reconnect.timer = None;
}

/// Ticks the reconnect timer and spawns a connection task when it fires.
fn tick_reconnect_timer<C, M>(
    time: Res<Time>,
    mut reconnect: ResMut<ReconnectBackoff>,
    config: Res<StdbConnectionConfig<C, M>>,
=======
/// Fully resets reconnect state when a connect succeeds.
fn on_connect(
    mut msgs: ReadStdbConnectedMessage,
    mut backoff: ResMut<ReconnectBackoff>,
    config: Res<ReconnectConfig>,
) {
    if msgs.read().next().is_some() {
        backoff.attempts = 0;
        backoff.current_delay = config.initial_delay;
        backoff.timer = None;
    }
}

/// Arms the reconnect timer on an unexpected disconnect or connection error.
///
/// A clean disconnect (no error) is treated as intentional and does not trigger
/// a reconnect. Initializes [`ReconnectBackoff::current_delay`] from
/// [`ReconnectConfig::initial_delay`] before the first attempt.
fn arm_reconnect_timer(
    mut disconnect_msgs: ReadStdbDisconnectedMessage,
    mut error_msgs: ReadStdbConnectErrorMessage,
    mut backoff: ResMut<ReconnectBackoff>,
    config: Res<ReconnectConfig>,
) {
    let unexpected_disconnect = disconnect_msgs.read().any(|msg| msg.err.is_some());
    let connect_error = error_msgs.read().next().is_some();

    if !(unexpected_disconnect || connect_error) {
        return;
    }

    if backoff.current_delay.is_zero() {
        backoff.current_delay = config.initial_delay;
    }
    backoff.timer = Some(Timer::new(backoff.current_delay, TimerMode::Once));
}

/// Ticks the reconnect timer and spawns a new connection attempt when it fires.
///
/// Pauses while a [`PendingConnection`] is already in-flight. Respects
/// [`ReconnectConfig::max_attempts`], and advances the delay by
/// [`ReconnectConfig::backoff_factor`] after each attempt.
fn tick_reconnect_timer<C, M>(
    time: Res<Time>,
    mut backoff: ResMut<ReconnectBackoff>,
    config: Res<ReconnectConfig>,
    conn_config: Res<StdbConnectionConfig<C, M>>,
>>>>>>> origin/main
    pending: Option<Res<PendingConnection<C>>>,
    mut commands: Commands,
) where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
<<<<<<< HEAD
    let Some(timer) = reconnect.timer.as_mut() else {
=======
    if backoff.timer.is_none() || pending.is_some() {
        return;
    }

    let Some(timer) = backoff.timer.as_mut() else {
>>>>>>> origin/main
        return;
    };

    timer.tick(time.delta());

<<<<<<< HEAD
    if timer.just_finished() {
        reconnect.timer = None;
        if pending.is_none() {
            let config = config.clone();
            let task = IoTaskPool::get().spawn(async move { config.build_connection().await });
            commands.insert_resource(PendingConnection::<C>(task));
        }
    }
=======
    if !timer.just_finished() {
        return;
    }

    backoff.timer = None;
    backoff.attempts += 1;

    if config.max_attempts > 0 && backoff.attempts > config.max_attempts {
        return;
    }

    let next_delay = backoff
        .current_delay
        .mul_f32(config.backoff_factor.max(1.0));
    backoff.current_delay = next_delay.min(config.max_delay);

    let conn_config = conn_config.clone();
    let task = IoTaskPool::get().spawn(async move { conn_config.build_connection().await });
    commands.insert_resource(PendingConnection::<C>(task));
>>>>>>> origin/main
}

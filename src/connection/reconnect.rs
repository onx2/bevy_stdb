//! Reconnect policy and runtime state for SpacetimeDB connections.
//!
//! Manages reconnect timing and backoff. When a disconnect with an error or a
//! disconnect error message is received, a reconnect timer is scheduled. When
//! the timer fires, a connection task is spawned directly.

use super::{PendingConnection, PendingConnectionPhase, StdbConnection, StdbConnectionConfig};
use crate::{
    alias::{ReadStdbConnectedMessage, ReadStdbDisconnectedMessage},
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{Commands, IntoScheduleConfigs, Res, ResMut, Resource};
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
    /// Multiplier applied after each failed reconnect attempt.
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

/// Runtime reconnect configuration.
#[derive(Resource, Clone)]
struct ReconnectConfig(pub StdbReconnectOptions);

impl Deref for ReconnectConfig {
    type Target = StdbReconnectOptions;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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
            (
                reset_reconnect_state,
                update_reconnect_backoff::<C>,
                tick_reconnect_timer::<C, M>,
            )
                .chain()
                .in_set(StdbSet::Connection),
        );
    }
}

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
    pending: Option<Res<PendingConnection<C>>>,
    mut commands: Commands,
) where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    let Some(timer) = reconnect.timer.as_mut() else {
        return;
    };

    timer.tick(time.delta());

    if timer.just_finished() {
        reconnect.timer = None;
        if pending.is_none() {
            let config = config.clone();
            let task = IoTaskPool::get().spawn(async move { config.build_connection().await });
            commands.insert_resource(PendingConnection::<C>::new(PendingConnectionPhase::Build(
                task,
            )));
        }
    }
}

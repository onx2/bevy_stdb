//! Reconnect policy and runtime state for SpacetimeDB connections.
//!
//! Manages reconnect timing and backoff. When the backoff timer fires,
//! a connection request is issued through [`StdbConnectionController`]
//! and the connection module handles the actual connection building.

use crate::connection::{StdbConnectionController, StdbConnectionState};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Res, ResMut, Resource};
use bevy_state::prelude::{NextState, OnEnter, in_state};
use bevy_time::{Time, Timer, TimerMode};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::time::Duration;

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
struct ReconnectConfig {
    /// Delay before the first reconnect attempt after a disconnect.
    initial_delay: Duration,
    /// Maximum number of reconnect attempts before giving up.
    ///
    /// `0` retries indefinitely.
    max_attempts: u32,
    /// Multiplier applied after each failed reconnect attempt.
    backoff_factor: f32,
    /// Maximum delay between reconnect attempts.
    max_delay: Duration,
}

impl From<StdbReconnectOptions> for ReconnectConfig {
    fn from(options: StdbReconnectOptions) -> Self {
        Self {
            initial_delay: options.initial_delay,
            max_attempts: options.max_attempts,
            backoff_factor: options.backoff_factor.max(1.0),
            max_delay: options.max_delay,
        }
    }
}

/// Runtime state for reconnect attempts.
#[derive(Resource)]
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

impl Default for ReconnectBackoff {
    fn default() -> Self {
        Self {
            active: false,
            attempts: 0,
            current_delay: Duration::ZERO,
            timer: None,
        }
    }
}

/// Internal plugin for reconnect timing and backoff.
pub(crate) struct ReconnectPlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    reconnect_options: StdbReconnectOptions,
    _marker: std::marker::PhantomData<(C, M)>,
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
            _marker: std::marker::PhantomData,
        }
    }
}

impl<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
> Plugin for ReconnectPlugin<C, M>
{
    fn build(&self, app: &mut App) {
        app.insert_resource(ReconnectConfig::from(self.reconnect_options.clone()));
        app.init_resource::<ReconnectBackoff>();

        app.add_systems(
            OnEnter(StdbConnectionState::Disconnected),
            on_enter_disconnected,
        );

        app.add_systems(
            OnEnter(StdbConnectionState::Connected),
            reset_reconnect_state,
        );

        app.add_systems(
            PreUpdate,
            tick_reconnect_timer.run_if(in_state(StdbConnectionState::Disconnected)),
        );
    }
}

/// Begins or advances a reconnect cycle on entering [`StdbConnectionState::Disconnected`].
///
/// Transitions to [`StdbConnectionState::Exhausted`] when the maximum number
/// of attempts has been reached.
fn on_enter_disconnected(
    reconnect_config: Res<ReconnectConfig>,
    mut reconnect: ResMut<ReconnectBackoff>,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
) {
    if reconnect.active {
        reconnect.attempts += 1;

        if reconnect_config.max_attempts > 0 && reconnect.attempts >= reconnect_config.max_attempts
        {
            reconnect.active = false;
            reconnect.timer = None;
            next_state.set(StdbConnectionState::Exhausted);
            return;
        }

        let next_delay = reconnect
            .current_delay
            .mul_f32(reconnect_config.backoff_factor);
        reconnect.current_delay = next_delay.min(reconnect_config.max_delay);
    } else {
        reconnect.active = true;
        reconnect.attempts = 0;
        reconnect.current_delay = reconnect_config.initial_delay;
    }

    reconnect.timer = Some(Timer::new(reconnect.current_delay, TimerMode::Once));
}

/// Resets reconnect state when a connection is successfully established.
fn reset_reconnect_state(mut reconnect: ResMut<ReconnectBackoff>) {
    reconnect.active = false;
    reconnect.attempts = 0;
    reconnect.current_delay = Duration::ZERO;
    reconnect.timer = None;
}

/// Ticks the reconnect timer and requests a connection when it fires.
fn tick_reconnect_timer(
    time: Res<Time>,
    mut reconnect: ResMut<ReconnectBackoff>,
    mut controller: ResMut<StdbConnectionController>,
) {
    let Some(timer) = reconnect.timer.as_mut() else {
        return;
    };

    timer.tick(time.delta());

    if timer.just_finished() {
        reconnect.timer = None;
        controller.connect();
    }
}

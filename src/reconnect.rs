//! Reconnect policy and runtime state for SpacetimeDB connections.
//!
//! Manages reconnect timing and backoff, and the Exhausted policy
//! state via Bevy systems.

use crate::connection::{StdbConnectionState, begin_connect};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Res, ResMut, Resource, World};
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
    /// If `None`, retries indefinitely.
    pub max_attempts: Option<u32>,
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
            max_attempts: None,
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
    /// If `None`, retries indefinitely.
    max_attempts: Option<u32>,
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
struct ReconnectState {
    attempts: u32,
    current_delay: Duration,
    timer: Option<Timer>,
    attempt_in_progress: bool,
}

impl Default for ReconnectState {
    fn default() -> Self {
        Self {
            attempts: 0,
            current_delay: Duration::ZERO,
            timer: None,
            attempt_in_progress: false,
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
        app.init_resource::<ReconnectState>();

        app.add_systems(
            OnEnter(StdbConnectionState::Disconnected),
            begin_reconnect_on_disconnect,
        );
        app.add_systems(
            OnEnter(StdbConnectionState::Connected),
            reset_reconnect_state,
        );

        app.add_systems(
            PreUpdate,
            tick_reconnect_timer::<C, M>.run_if(in_state(StdbConnectionState::Disconnected)),
        );
    }
}

fn begin_reconnect_on_disconnect(
    reconnect_config: Res<ReconnectConfig>,
    mut reconnect: ResMut<ReconnectState>,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
) {
    if reconnect.attempt_in_progress {
        reconnect.attempt_in_progress = false;
        reconnect.attempts += 1;

        if let Some(max_attempts) = reconnect_config.max_attempts
            && reconnect.attempts >= max_attempts
        {
            reconnect.timer = None;
            next_state.set(StdbConnectionState::Exhausted);
            return;
        }

        let next_delay = reconnect
            .current_delay
            .mul_f32(reconnect_config.backoff_factor);
        reconnect.current_delay = next_delay.min(reconnect_config.max_delay);
        reconnect.timer = Some(Timer::new(reconnect.current_delay, TimerMode::Once));
        return;
    }

    if reconnect.timer.is_some() {
        return;
    }

    reconnect.attempts = 0;
    reconnect.current_delay = reconnect_config.initial_delay;
    reconnect.timer = Some(Timer::new(reconnect.current_delay, TimerMode::Once));
}

fn tick_reconnect_timer<C, M>(world: &mut World)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    if !ready_to_retry(world) {
        return;
    }

    begin_connect::<C, M>(world);

    world
        .get_resource_mut::<ReconnectState>()
        .expect("ReconnectState should exist before reconnect attempt")
        .attempt_in_progress = true;

    world
        .get_resource_mut::<NextState<StdbConnectionState>>()
        .expect("NextState<StdbConnectionState> should exist before reconnect attempt")
        .set(StdbConnectionState::Connecting);
}

fn ready_to_retry(world: &mut World) -> bool {
    let delta = world
        .get_resource::<Time>()
        .expect("Time resource should exist before reconnect ticking")
        .delta();

    let mut reconnect = world
        .get_resource_mut::<ReconnectState>()
        .expect("ReconnectState should exist before reconnect ticking");

    let Some(timer) = reconnect.timer.as_mut() else {
        return false;
    };

    timer.tick(delta);
    if !timer.is_finished() {
        return false;
    }

    reconnect.timer = None;
    true
}

fn reset_reconnect_state(mut reconnect: ResMut<ReconnectState>) {
    reconnect.attempts = 0;
    reconnect.current_delay = Duration::ZERO;
    reconnect.timer = None;
    reconnect.attempt_in_progress = false;
}

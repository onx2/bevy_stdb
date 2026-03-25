//! Reconnect policy and runtime state for SpacetimeDB connections.
//!
//! This module manages reconnect timing and backoff through Bevy systems.
//!
//! `Connected` and `Disconnected` are treated as SDK-driven states and should
//! only be entered in response to the connection callbacks forwarded through
//! `StdbConnectedMessage`, `StdbDisconnectedMessage`, and
//! `StdbConnectionErrorMessage`.
//!
//! The reconnect plugin owns the policy-oriented states instead:
//! - `Reconnecting` while retry attempts are pending
//! - `Exhausted` when retry attempts have been exhausted

use crate::connection::{StdbConnection, StdbConnectionConfig, StdbConnectionState};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Res, ResMut, Resource, World};
use bevy_state::prelude::{NextState, OnEnter, in_state};
use bevy_time::{Time, Timer, TimerMode};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::{sync::Arc, thread::JoinHandle, time::Duration};

type ReconnectAttempt<C> = Result<(Arc<C>, fn(&C) -> JoinHandle<()>), ()>;

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
pub(crate) struct ReconnectConfig {
    /// Delay before the first reconnect attempt after a disconnect.
    pub initial_delay: Duration,
    /// Maximum number of reconnect attempts before giving up.
    ///
    /// If `None`, retries indefinitely.
    pub max_attempts: Option<u32>,
    /// Multiplier applied after each failed reconnect attempt.
    pub backoff_factor: f32,
    /// Maximum delay between reconnect attempts.
    pub max_delay: Duration,
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
pub(crate) struct ReconnectState {
    pub attempts: u32,
    pub current_delay: Duration,
    pub timer: Option<Timer>,
}

impl Default for ReconnectState {
    fn default() -> Self {
        Self {
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
    config: ReconnectConfig,
    _marker: std::marker::PhantomData<(C, M)>,
}

impl<C, M> ReconnectPlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    /// Creates a reconnect plugin from the given options.
    pub fn new(options: StdbReconnectOptions) -> Self {
        Self {
            config: options.into(),
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
        app.insert_resource(self.config.clone());
        app.init_resource::<ReconnectState>();

        app.add_systems(
            OnEnter(StdbConnectionState::Disconnected),
            begin_reconnect_on_disconnect,
        );

        app.add_systems(
            PreUpdate,
            tick_reconnect_timer::<C, M>.run_if(in_state(StdbConnectionState::Reconnecting)),
        );
    }
}

fn begin_reconnect_on_disconnect(
    reconnect_config: Res<ReconnectConfig>,
    mut reconnect: ResMut<ReconnectState>,
    mut next_state: ResMut<NextState<StdbConnectionState>>,
) {
    reconnect.attempts = 0;
    reconnect.current_delay = reconnect_config.initial_delay;
    reconnect.timer = Some(Timer::new(reconnect.current_delay, TimerMode::Once));

    next_state.set(StdbConnectionState::Reconnecting);
}

fn tick_reconnect_timer<C, M>(world: &mut World)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    if !ready_to_retry(world) {
        return;
    }

    match try_reconnect::<C, M>(world) {
        Ok((conn, run_fn)) => on_reconnect_success(world, conn, run_fn),
        Err(_) => on_reconnect_failure(world),
    }
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
    timer.is_finished()
}

fn try_reconnect<C, M>(world: &mut World) -> ReconnectAttempt<C>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    let config = world
        .get_resource::<StdbConnectionConfig<C, M>>()
        .expect("StdbConnectionConfig should exist during reconnect");

    let run_fn = config.run_fn;
    match config.build_connection() {
        Ok(conn) => Ok((conn, run_fn)),
        Err(_) => Err(()),
    }
}

fn on_reconnect_success<C>(world: &mut World, conn: Arc<C>, run_fn: fn(&C) -> JoinHandle<()>)
where
    C: DbContext + Send + Sync + 'static,
{
    run_fn(conn.as_ref());
    world.insert_resource(StdbConnection::new(conn));

    {
        let mut reconnect = world
            .get_resource_mut::<ReconnectState>()
            .expect("ReconnectState should exist during reconnect success");
        reconnect.attempts = 0;
        reconnect.current_delay = Duration::ZERO;
        reconnect.timer = None;
    }
}

fn on_reconnect_failure(world: &mut World) {
    let reconnect_config = world
        .get_resource::<ReconnectConfig>()
        .expect("ReconnectConfig should exist during reconnect failure")
        .clone();

    let mut reconnect = world
        .get_resource_mut::<ReconnectState>()
        .expect("ReconnectState should exist during reconnect failure");

    reconnect.attempts += 1;

    if let Some(max_attempts) = reconnect_config.max_attempts
        && reconnect.attempts >= max_attempts
    {
        reconnect.timer = None;

        world
            .get_resource_mut::<NextState<StdbConnectionState>>()
            .expect("NextState<StdbConnectionState> should exist during reconnect exhaustion")
            .set(StdbConnectionState::Exhausted);
        return;
    }

    let next_delay = reconnect
        .current_delay
        .mul_f32(reconnect_config.backoff_factor);
    reconnect.current_delay = next_delay.min(reconnect_config.max_delay);
    reconnect.timer = Some(Timer::new(reconnect.current_delay, TimerMode::Once));
}

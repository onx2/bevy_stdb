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

use crate::connection::{
    ConnectionDriver, StdbConnection, StdbConnectionConfig, StdbConnectionState,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Res, ResMut, Resource, World};
use bevy_state::prelude::{NextState, OnEnter, in_state};
use bevy_time::{Time, Timer, TimerMode};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
#[cfg(feature = "browser")]
use std::sync::{
    Mutex,
    mpsc::{Receiver, TryRecvError, channel},
};
use std::{sync::Arc, time::Duration};

/// Browser-only reconnect result type.
type ReconnectResult<C> = Result<(Arc<C>, Option<ConnectionDriver<C>>), ()>;

/// Browser-only receiver for an in-flight reconnect attempt.
///
/// The browser reconnect path builds the replacement connection asynchronously
/// and sends the result back through this resource so runtime systems can
/// finalize success or failure on the Bevy world thread.
#[cfg(feature = "browser")]
#[derive(Resource)]
pub(crate) struct PendingReconnectReceiver<C: DbContext + Send + Sync + 'static> {
    pub rx: Mutex<Receiver<ReconnectResult<C>>>,
}

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
    reconnect_options: StdbReconnectOptions,
    _marker: std::marker::PhantomData<(C, M)>,
}

impl<C, M> ReconnectPlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    /// Creates a reconnect plugin from the given options.
    pub fn new(reconnect_options: StdbReconnectOptions) -> Self {
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
            PreUpdate,
            (
                tick_reconnect_timer::<C, M>.run_if(in_state(StdbConnectionState::Reconnecting)),
                #[cfg(feature = "browser")]
                finalize_pending_reconnect::<C>.run_if(in_state(StdbConnectionState::Reconnecting)),
            ),
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

#[cfg(not(feature = "browser"))]
fn tick_reconnect_timer<C, M>(world: &mut World)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    if !ready_to_retry(world) {
        return;
    }

    match try_reconnect::<C, M>(world) {
        Ok((conn, driver)) => on_reconnect_success(world, conn, driver),
        Err(_) => on_reconnect_failure(world),
    }
}

#[cfg(feature = "browser")]
fn tick_reconnect_timer<C, M>(world: &mut World)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    if !ready_to_retry(world) {
        return;
    }

    if world.contains_resource::<PendingReconnectReceiver<C>>() {
        return;
    }

    let config = world
        .get_resource::<StdbConnectionConfig<C, M>>()
        .expect("StdbConnectionConfig should exist during reconnect")
        .clone();

    let (tx, rx) = channel();

    wasm_bindgen_futures::spawn_local(async move {
        let result = match config.build_connection().await {
            Ok(conn) => Ok((conn, config.driver.clone())),
            Err(_) => Err(()),
        };
        let _ = tx.send(result);
    });

    world.insert_resource(PendingReconnectReceiver::<C> { rx: Mutex::new(rx) });
}

#[cfg(not(feature = "browser"))]
fn try_reconnect<C, M>(world: &mut World) -> ReconnectResult<C>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    let config = world
        .get_resource::<StdbConnectionConfig<C, M>>()
        .expect("StdbConnectionConfig should exist during reconnect");

    let driver = config.driver.clone();
    match config.build_connection() {
        Ok(conn) => Ok((conn, driver)),
        Err(_) => Err(()),
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

fn on_reconnect_success<C>(world: &mut World, conn: Arc<C>, driver: Option<ConnectionDriver<C>>)
where
    C: DbContext + Send + Sync + 'static,
{
    if let Some(ConnectionDriver::Background(background_driver)) = driver {
        background_driver(conn.as_ref());
    }
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

#[cfg(feature = "browser")]
fn finalize_pending_reconnect<C>(world: &mut World)
where
    C: DbContext + Send + Sync + 'static,
{
    let result = {
        let Some(receiver) = world.get_resource::<PendingReconnectReceiver<C>>() else {
            return;
        };

        let rx = receiver.rx.lock().unwrap_or_else(|e| e.into_inner());

        match rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            // The receiver is per reconnect attempt.
            // If its sender drops before sending a result, treat that attempt as a normal reconnect failure.
            Err(TryRecvError::Disconnected) => Some(Err(())),
        }
    };

    let Some(result) = result else {
        return;
    };

    world.remove_resource::<PendingReconnectReceiver<C>>();
    match result {
        Ok((conn, driver)) => on_reconnect_success(world, conn, driver),
        Err(_) => on_reconnect_failure(world),
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

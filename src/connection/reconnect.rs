//! Reconnect policy and runtime state for SpacetimeDB connections.
//!
//! Manages reconnect timing and backoff. When a disconnect is received the
//! reconnect cycle activates. Each tick the timer is advanced and, once it
//! fires and no [`PendingConnection`] is in-flight, a new connection attempt is
//! requested. A successful connect resets the cycle.

use super::{PendingConnection, StdbConnection};
use crate::{
    alias::{ReadStdbConnectErrorMessage, ReadStdbConnectedMessage, ReadStdbDisconnectedMessage},
    commands::{StartConnectCommand, StdbConnectOptions},
    message::StdbDisconnectRequestedMessage,
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{
    Commands, IntoScheduleConfigs, MessageReader, Res, ResMut, Resource, not, resource_exists,
};
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
    /// Multiplier applied to the current delay after each failed attempt.
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

/// Runtime reconnect configuration resource.
#[derive(Resource, Clone)]
struct ReconnectConfig(pub StdbReconnectOptions);

impl Deref for ReconnectConfig {
    type Target = StdbReconnectOptions;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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
            (on_connect, arm_reconnect_timer::<C>).in_set(StdbSet::Connection),
        );

        app.add_systems(
            PreUpdate,
            tick_reconnect_timer::<C, M>
                .run_if(not(resource_exists::<StdbConnection<C>>))
                .run_if(|backoff: Res<ReconnectBackoff>| backoff.timer.is_some())
                .run_if(not(resource_exists::<PendingConnection<C>>))
                .in_set(StdbSet::Connection),
        );
    }
}

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
/// A disconnect requested through the connection commands is treated as intentional; an unmarked
/// disconnect is retried even when the SDK provides no error. Initializes
/// [`ReconnectBackoff::current_delay`] from [`ReconnectConfig::initial_delay`] before the first attempt.
fn arm_reconnect_timer<C: DbContext + Send + Sync + 'static>(
    mut disconnect_msgs: ReadStdbDisconnectedMessage,
    mut error_msgs: ReadStdbConnectErrorMessage,
    mut requested_disconnects: MessageReader<StdbDisconnectRequestedMessage>,
    mut backoff: ResMut<ReconnectBackoff>,
    config: Res<ReconnectConfig>,
) {
    let disconnected = disconnect_msgs.read().next().is_some();
    let intentional_disconnect = requested_disconnects.read().next().is_some();
    let connect_error = error_msgs.read().next().is_some();

    if intentional_disconnect {
        return;
    }

    if !(connect_error || disconnected) {
        return;
    }

    if backoff.current_delay.is_zero() {
        backoff.current_delay = config.initial_delay;
    }
    backoff.timer = Some(Timer::new(backoff.current_delay, TimerMode::Once));
}

/// Ticks the reconnect timer and requests a connection attempt when it fires.
///
/// Respects [`ReconnectConfig::max_attempts`] and [`ReconnectConfig::backoff_factor`].
fn tick_reconnect_timer<C, M>(
    time: Res<Time>,
    mut backoff: ResMut<ReconnectBackoff>,
    config: Res<ReconnectConfig>,
    mut commands: Commands,
) where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    let Some(timer) = backoff.timer.as_mut() else {
        return;
    };

    timer.tick(time.delta());

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

    commands.queue(StartConnectCommand::<C, M>::new(
        StdbConnectOptions::default(),
    ));
}

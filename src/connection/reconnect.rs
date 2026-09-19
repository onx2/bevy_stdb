//! Reconnect policy and runtime state for SpacetimeDB connections.
//!
//! Manages reconnect timing and backoff. When a disconnect is received the
//! reconnect cycle activates. Each tick the timer is advanced and, once it
//! fires and no [`PendingConnection`] is in-flight, a new connection attempt is
//! requested. A successful connect resets the cycle.

use super::{PendingConnection, StdbConnection};
use crate::{
    commands::{StartConnectCommand, StdbConnectOptions},
    message::StdbReconnectExhaustedMessage,
    reader::{ReadStdbConnectErrorMessage, ReadStdbConnectedMessage, ReadStdbDisconnectedMessage},
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{
    Commands, IntoScheduleConfigs, MessageWriter, Res, ResMut, Resource, not, resource_exists,
};
use bevy_time::{Real, Time, Timer, TimerMode};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::{marker::PhantomData, time::Duration};

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
struct ReconnectConfig {
    options: StdbReconnectOptions,
    /// Queues a connection attempt. A plain function so the timer systems carry none of the
    /// connection generics, and a test can watch for the attempt without a connection type.
    start_attempt: fn(&mut Commands),
}

/// What [`ReconnectBackoff::arm`] decided.
#[derive(Debug, PartialEq, Eq)]
enum Armed {
    /// The timer is counting down to the next attempt.
    Waiting,
    /// Every allowed attempt has been made, so no timer was armed.
    Exhausted,
}

/// Runtime state for the active reconnect cycle.
///
/// The presence of `timer` signals that a reconnect cycle is active.
#[derive(Resource, Default)]
struct ReconnectBackoff {
    /// Number of reconnect attempts made in the current cycle.
    attempts: u32,
    /// Delay that will be used for the next reconnect attempt. Zero until the cycle first arms.
    current_delay: Duration,
    /// Countdown timer for the next reconnect attempt.
    timer: Option<Timer>,
}

impl ReconnectBackoff {
    /// Starts the countdown to the next attempt, unless the attempt budget is spent.
    fn arm(&mut self, options: &StdbReconnectOptions) -> Armed {
        if options.max_attempts > 0 && self.attempts >= options.max_attempts {
            self.timer = None;
            return Armed::Exhausted;
        }

        if self.current_delay.is_zero() {
            self.current_delay = options.initial_delay;
        }
        self.timer = Some(Timer::new(self.current_delay, TimerMode::Once));
        Armed::Waiting
    }

    /// Advances the countdown, returning `true` when an attempt is due.
    ///
    /// A due attempt is counted and grows the delay the next [`Self::arm`] will use.
    fn tick(&mut self, delta: Duration, options: &StdbReconnectOptions) -> bool {
        let Some(timer) = self.timer.as_mut() else {
            return false;
        };
        if !timer.tick(delta).just_finished() {
            return false;
        }

        self.timer = None;
        self.attempts += 1;
        self.current_delay = self
            .current_delay
            .mul_f32(options.backoff_factor.max(1.0))
            .min(options.max_delay);
        true
    }
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
        app.add_message::<StdbReconnectExhaustedMessage>();
        app.insert_resource(ReconnectConfig {
            options: self.reconnect_options.clone(),
            start_attempt: |commands| {
                commands.queue(StartConnectCommand::<C, M>::new(
                    StdbConnectOptions::default(),
                ));
            },
        });
        app.init_resource::<ReconnectBackoff>();

        app.add_systems(
            PreUpdate,
            (on_connect, arm_reconnect_timer).in_set(StdbSet::Connection),
        );

        // The countdown is paused while a connection is live or an attempt is in flight.
        app.add_systems(
            PreUpdate,
            tick_reconnect_timer
                .run_if(not(resource_exists::<StdbConnection<C>>))
                .run_if(not(resource_exists::<PendingConnection<C>>))
                .in_set(StdbSet::Connection),
        );
    }
}

/// Fully resets reconnect state when a connect succeeds.
fn on_connect(mut msgs: ReadStdbConnectedMessage, mut backoff: ResMut<ReconnectBackoff>) {
    if msgs.read().count() > 0 {
        *backoff = ReconnectBackoff::default();
    }
}

/// Arms the reconnect timer when a connection is lost or an attempt fails.
///
/// Only a disconnect this client asked for is left alone. The SDK reports a server going away as
/// a disconnect with no error, exactly as it reports a requested one, so the absence of an error
/// says nothing; [`StdbDisconnectedMessage::was_requested`] is what tells them apart.
///
/// [`StdbDisconnectedMessage::was_requested`]: crate::message::StdbDisconnectedMessage::was_requested
fn arm_reconnect_timer(
    mut disconnect_msgs: ReadStdbDisconnectedMessage,
    mut error_msgs: ReadStdbConnectErrorMessage,
    mut backoff: ResMut<ReconnectBackoff>,
    mut exhausted: MessageWriter<StdbReconnectExhaustedMessage>,
    config: Res<ReconnectConfig>,
) {
    // `count`, not `any`: a reader only advances past what its iterator yielded.
    let lost = disconnect_msgs
        .read()
        .filter(|msg| !msg.was_requested())
        .count();
    let failed = error_msgs.read().count();

    if lost + failed == 0 {
        return;
    }

    if backoff.arm(&config.options) == Armed::Exhausted {
        exhausted.write(StdbReconnectExhaustedMessage {
            attempts: backoff.attempts,
        });
    }
}

/// Ticks the reconnect timer and requests a connection attempt when it fires.
///
/// Counts wall-clock time. The default `Time` here is `Time<Virtual>`, which stops while the game
/// is paused, scales with game speed, and clamps a long frame to a quarter of a second; a network
/// retry should do none of those.
fn tick_reconnect_timer(
    time: Res<Time<Real>>,
    mut backoff: ResMut<ReconnectBackoff>,
    config: Res<ReconnectConfig>,
    mut commands: Commands,
) {
    if backoff.tick(time.delta(), &config.options) {
        (config.start_attempt)(&mut commands);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{DisconnectIntent, StdbConnectedMessage, StdbDisconnectedMessage};
    use bevy_app::App;
    use bevy_ecs::prelude::{Messages, World};
    use bevy_time::{TimePlugin, TimeUpdateStrategy, Virtual};
    use spacetimedb_sdk::Identity;

    const SECOND: Duration = Duration::from_secs(1);

    fn options(max_attempts: u32) -> StdbReconnectOptions {
        StdbReconnectOptions {
            initial_delay: SECOND,
            max_attempts,
            backoff_factor: 2.0,
            max_delay: Duration::from_secs(5),
        }
    }

    /// Counts the attempts the timer asked for, standing in for `StartConnectCommand`.
    #[derive(Resource, Default)]
    struct Attempts(u32);

    /// The reconnect systems as [`ReconnectPlugin`] schedules them, minus the run conditions
    /// that need a connection type. Each `update` advances every clock by one second.
    fn app(max_attempts: u32) -> App {
        let mut app = App::new();
        app.add_plugins(TimePlugin);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(SECOND));
        app.add_message::<StdbConnectedMessage>();
        app.add_message::<StdbDisconnectedMessage>();
        app.add_message::<crate::message::StdbConnectErrorMessage>();
        app.add_message::<StdbReconnectExhaustedMessage>();
        app.init_resource::<Attempts>();
        app.init_resource::<ReconnectBackoff>();
        app.insert_resource(ReconnectConfig {
            options: options(max_attempts),
            start_attempt: |commands| {
                commands.queue(|world: &mut World| world.resource_mut::<Attempts>().0 += 1);
            },
        });
        app.add_systems(
            PreUpdate,
            (on_connect, arm_reconnect_timer, tick_reconnect_timer).chain(),
        );
        // `ManualDuration` takes effect from the second update.
        app.update();
        app
    }

    fn disconnect(app: &mut App, result: Result<DisconnectIntent, spacetimedb_sdk::Error>) {
        app.world_mut()
            .write_message(StdbDisconnectedMessage { result });
    }

    fn attempts(app: &App) -> u32 {
        app.world().resource::<Attempts>().0
    }

    #[test]
    fn the_delay_grows_by_the_backoff_factor_up_to_the_cap() {
        let options = options(0);
        let mut backoff = ReconnectBackoff::default();
        let mut delays = Vec::new();

        for _ in 0..5 {
            assert_eq!(backoff.arm(&options), Armed::Waiting);
            delays.push(backoff.current_delay.as_secs());
            assert!(backoff.tick(backoff.current_delay, &options));
        }

        assert_eq!(delays, [1, 2, 4, 5, 5]);
    }

    #[test]
    fn a_timer_that_has_not_run_out_asks_for_nothing() {
        let options = options(0);
        let mut backoff = ReconnectBackoff::default();
        backoff.arm(&options);

        assert!(!backoff.tick(SECOND / 2, &options));
        assert_eq!(backoff.attempts, 0);
    }

    #[test]
    fn arming_stops_once_the_attempt_budget_is_spent() {
        let options = options(2);
        let mut backoff = ReconnectBackoff::default();

        for _ in 0..2 {
            assert_eq!(backoff.arm(&options), Armed::Waiting);
            assert!(backoff.tick(Duration::from_secs(60), &options));
        }

        assert_eq!(backoff.arm(&options), Armed::Exhausted);
        assert!(!backoff.tick(Duration::from_secs(60), &options));
        assert_eq!(backoff.attempts, 2);
    }

    #[test]
    fn a_lost_connection_is_retried_even_though_the_sdk_reports_no_error() {
        // The SDK bug this works around: a server going away arrives as a disconnect with no
        // error, which used to read as intentional and was never retried.
        let mut app = app(0);
        disconnect(&mut app, Ok(DisconnectIntent::Lost));
        app.update();
        app.update();

        assert_eq!(attempts(&app), 1);
    }

    #[test]
    fn a_requested_disconnect_is_not_retried() {
        let mut app = app(0);
        disconnect(&mut app, Ok(DisconnectIntent::Requested));
        for _ in 0..5 {
            app.update();
        }

        assert_eq!(attempts(&app), 0);
    }

    #[test]
    fn a_requested_disconnect_does_not_hide_a_lost_one_in_the_same_frame() {
        let mut app = app(0);
        disconnect(&mut app, Ok(DisconnectIntent::Requested));
        disconnect(&mut app, Ok(DisconnectIntent::Lost));
        app.update();
        app.update();

        assert_eq!(attempts(&app), 1);
    }

    #[test]
    fn a_successful_connect_restores_the_full_budget_and_the_first_delay() {
        let mut app = app(1);
        disconnect(&mut app, Ok(DisconnectIntent::Lost));
        app.update();
        app.update();
        assert_eq!(attempts(&app), 1);

        app.world_mut().write_message(StdbConnectedMessage {
            identity: Identity::from_byte_array([0; 32]),
            access_token: String::new(),
        });
        app.update();

        let backoff = app.world().resource::<ReconnectBackoff>();
        assert_eq!(
            (backoff.attempts, backoff.current_delay),
            (0, Duration::ZERO)
        );
    }

    #[test]
    fn running_out_of_attempts_is_reported_once_per_failure() {
        let mut app = app(1);
        disconnect(&mut app, Ok(DisconnectIntent::Lost));
        app.update();
        app.update();
        assert_eq!(attempts(&app), 1);

        // The one allowed attempt failed.
        disconnect(&mut app, Ok(DisconnectIntent::Lost));
        app.update();

        let exhausted = app
            .world()
            .resource::<Messages<StdbReconnectExhaustedMessage>>();
        assert_eq!(exhausted.len(), 1);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(attempts(&app), 1);
    }

    #[test]
    fn the_backoff_runs_on_real_time_so_a_paused_game_still_reconnects() {
        // `Time` in `PreUpdate` is `Time<Virtual>`. A pause menu pauses it, and a connection lost
        // behind that menu must still come back.
        let mut app = app(0);
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        disconnect(&mut app, Ok(DisconnectIntent::Lost));
        for _ in 0..3 {
            app.update();
        }

        assert_eq!(app.world().resource::<Time<Real>>().delta(), SECOND);
        assert_eq!(attempts(&app), 1);
    }

    #[test]
    fn the_backoff_ignores_game_speed() {
        // At 10x a one second delay would otherwise elapse in a tenth of a second.
        let mut app = app(0);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(SECOND / 10));
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_relative_speed(10.0);
        disconnect(&mut app, Ok(DisconnectIntent::Lost));
        for _ in 0..3 {
            app.update();
        }

        assert_eq!(attempts(&app), 0);
    }
}

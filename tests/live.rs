//! End-to-end checks against a real SpacetimeDB. Ignored by default; to run them:
//!
//! ```sh
//! spacetime start
//! spacetime publish --server local --module-path examples/simple/server bevy-stdb-simple
//! cargo test --test live -- --ignored --test-threads 1
//! ```
// The generated bindings select their native API by target, the SDK by feature, so they cannot
// compile natively once `--all-features` turns `browser` on.
#![cfg(not(feature = "browser"))]
mod support;

use bevy_app::{App, First, TaskPoolPlugin, Update};
use bevy_ecs::prelude::*;
use bevy_stdb::prelude::*;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use support::module_bindings::{
    DbConnection, Player, PlayerTableAccess, PlayerTableAccessor, RemoteModule, move_player,
};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Players;

type Conn = StdbConnection<DbConnection>;
type Subs = StdbSubscriptions<Players, RemoteModule>;
type Cmds<'w, 's> = StdbCommands<'w, 's, DbConnection, RemoteModule>;

fn subscribe_on_connect(mut connected: ReadStdbConnectedMessage, mut subs: ResMut<Subs>) {
    if connected.read().count() > 0 {
        subs.subscribe_sql(Players, "SELECT * FROM player");
    }
}

/// Runs frames until `done`, failing the test after `limit`.
fn run_until(app: &mut App, limit: Duration, what: &str, mut done: impl FnMut(&mut App) -> bool) {
    let start = Instant::now();
    while !done(app) {
        assert!(
            start.elapsed() < limit,
            "timed out waiting for {what}: {:?}",
            app.world().get_resource::<Log>()
        );
        app.update();
        std::thread::sleep(Duration::from_millis(2));
    }
}

const UNSET: u64 = u64::MAX;
static FRAME: AtomicU64 = AtomicU64::new(0);
static SDK_CALLBACK_FRAME: AtomicU64 = AtomicU64::new(UNSET);
static READER_FRAME: AtomicU64 = AtomicU64::new(UNSET);
const MARKER_X: f32 = 4242.5;

#[test]
#[ignore = "needs a local SpacetimeDB with the example module published"]
fn a_frame_driven_row_is_read_in_the_frame_its_callback_ran() {
    use spacetimedb_sdk::TableWithPrimaryKey;

    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default());
    app.add_plugins(
        support::target()
            .with_frame_driver(DbConnection::frame_tick)
            .with_eager_connection()
            .with_subscriptions::<Players>()
            .add_table::<PlayerTableAccessor>(),
    );
    app.add_systems(First, || {
        FRAME.fetch_add(1, Ordering::SeqCst);
    });
    app.add_systems(
        Update,
        (
            subscribe_on_connect,
            // A callback registered straight on the SDK records the frame `frame_tick` ran it in.
            |mut connected: ReadStdbConnectedMessage, conn: Option<Res<Conn>>| {
                if connected.read().count() > 0 {
                    conn.unwrap().db().player().on_update(|_, _, new| {
                        if new.x == MARKER_X {
                            SDK_CALLBACK_FRAME
                                .store(FRAME.load(Ordering::SeqCst), Ordering::SeqCst);
                        }
                    });
                }
            },
            |mut applied: ReadStdbSubscriptionAppliedMessage<Players>, conn: Option<Res<Conn>>| {
                if applied.read().count() > 0 {
                    conn.unwrap().reducers().move_player(MARKER_X, 0.0).unwrap();
                }
            },
            |mut updates: ReadUpdateMessage<Player>| {
                for update in updates.read() {
                    if update.new.x == MARKER_X {
                        READER_FRAME.store(FRAME.load(Ordering::SeqCst), Ordering::SeqCst);
                    }
                }
            },
        ),
    );

    run_until(&mut app, Duration::from_secs(10), "the moved row", |_| {
        READER_FRAME.load(Ordering::SeqCst) != UNSET
    });

    assert_eq!(
        READER_FRAME.load(Ordering::SeqCst),
        SDK_CALLBACK_FRAME.load(Ordering::SeqCst),
        "the reader saw the row in a later frame than the SDK callback delivered it"
    );
}

#[derive(Resource, Default, Debug)]
struct Log {
    connected: usize,
    applied: usize,
    inserts: usize,
    disconnects: Vec<String>,
}

fn record(
    mut log: ResMut<Log>,
    mut connected: ReadStdbConnectedMessage,
    mut applied: ReadStdbSubscriptionAppliedMessage<Players>,
    mut inserts: ReadInsertMessage<Player>,
    mut disconnected: ReadStdbDisconnectedMessage,
) {
    log.connected += connected.read().count();
    log.applied += applied.read().count();
    log.inserts += inserts.read().count();
    for msg in disconnected.read() {
        log.disconnects.push(format!("{:?}", msg.result));
    }
}

fn background_app() -> App {
    recording_app(support::plugin())
}

fn frame_driven_app() -> App {
    recording_app(support::target().with_frame_driver(DbConnection::frame_tick))
}

fn recording_app(plugin: support::Plugin) -> App {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), bevy_time::TimePlugin));
    app.add_plugins(
        plugin
            .with_eager_connection()
            .with_subscriptions::<Players>()
            .with_reconnect(StdbReconnectOptions {
                initial_delay: Duration::from_millis(200),
                ..Default::default()
            })
            .add_table::<PlayerTableAccessor>(),
    );
    app.init_resource::<Log>();
    app.add_systems(Update, (subscribe_on_connect, record).chain());
    app
}

#[test]
#[ignore = "needs a local SpacetimeDB with the example module published"]
fn a_requested_reconnect_subscribes_once_on_the_new_connection() {
    requested_reconnect_subscribes_once(background_app());
}

#[test]
#[ignore = "needs a local SpacetimeDB with the example module published"]
fn a_frame_driven_requested_reconnect_subscribes_once_on_the_new_connection() {
    // Once replaced, a frame-driven connection is never ticked again, so nothing can be left
    // waiting on its disconnect callback.
    requested_reconnect_subscribes_once(frame_driven_app());
}

fn requested_reconnect_subscribes_once(mut app: App) {
    run_until(
        &mut app,
        Duration::from_secs(10),
        "the first subscription",
        |app| app.world().resource::<Log>().applied == 1,
    );

    app.world_mut()
        .run_system_cached(|mut cmds: Cmds| cmds.reconnect(StdbConnectOptions::default()))
        .unwrap();
    run_until(
        &mut app,
        Duration::from_secs(10),
        "the second connection",
        |app| {
            let log = app.world().resource::<Log>();
            log.connected == 2 && log.applied >= 2
        },
    );
    // Leave room for a duplicate subscription to be applied.
    let settle = Instant::now();
    run_until(&mut app, Duration::from_secs(5), "settling", |_| {
        settle.elapsed() > Duration::from_secs(1)
    });

    let log = app.world().resource::<Log>();
    assert!(log.disconnects.iter().all(|d| d == "Ok(Requested)"));
    assert_eq!(log.applied, 2, "one subscription per connection");
}

#[test]
#[ignore = "restart the local SpacetimeDB while this runs"]
fn a_server_restart_is_reported_as_lost_and_recovered_from() {
    let mut app = background_app();
    run_until(
        &mut app,
        Duration::from_secs(10),
        "the first subscription",
        |app| app.world().resource::<Log>().applied == 1,
    );
    let inserts_before = app.world().resource::<Log>().inserts;
    assert!(inserts_before > 0);
    eprintln!("connected; restart the server now");

    run_until(&mut app, Duration::from_secs(120), "the reconnect", |app| {
        let log = app.world().resource::<Log>();
        log.connected == 2 && log.applied == 2 && log.inserts > inserts_before
    });

    let log = app.world().resource::<Log>();
    eprintln!("disconnects: {:?}", log.disconnects);
    assert!(!log.disconnects.is_empty());
    assert!(log.disconnects.iter().all(|d| d != "Ok(Requested)"));
}

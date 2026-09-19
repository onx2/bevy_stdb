//! Per-frame cost of delivering table changes to the readers an app binds.
//!
//! - `frame`: what the Bevy thread pays. `n` changes are already in the channel when the clock
//!   starts, so an iteration is `app.update()` alone: the drain, then every reader system.
//!   Bindings are matched to readers, since an app binds the capabilities it reads and no more.
//! - `produce`: what the SDK's thread pays per change downstream of the SDK -- building the
//!   `TableChange` and sending it. In an app that is off the frame entirely, which is why it is
//!   kept out of `frame`.
//! - `idle`: a frame with nothing to deliver, with one table and with a hundred more channels, so
//!   the fixed cost of every registered channel is visible.
//!
//! Everything upstream of the send needs a live connection, so it is not here.
#[path = "../src/module_bindings/mod.rs"]
mod module_bindings;

use bevy::prelude::*;
use bevy_stdb::prelude::*;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use module_bindings::{DbConnection, Player, PlayerTableAccessor, Reducer, RemoteModule};
use spacetimedb_sdk::{Event, Identity};
use std::hint::black_box;
use std::sync::Arc;

/// What an app binds, and therefore which row callbacks deliver anything.
#[derive(Clone, Copy)]
enum Bound {
    /// One capability, the floor.
    Insert,
    /// What a mirror binds: upserts plus deletes.
    Mirror,
    /// Everything `add_table` gives a table with a primary key.
    All,
}

impl Bound {
    fn name(self) -> &'static str {
        match self {
            Self::Insert => "bind_insert",
            Self::Mirror => "insert_update + delete",
            Self::All => "add_table",
        }
    }
}

fn read_inserts(mut r: ReadInsertMessage<Player>) {
    for m in r.read() {
        black_box(m.row.x);
    }
}

fn read_deletes(mut r: ReadDeleteMessage<Player>) {
    for m in r.read() {
        black_box(m.row.x);
    }
}

fn read_updates(mut r: ReadUpdateMessage<Player>) {
    for m in r.read() {
        black_box(m.new.x);
    }
}

fn read_upserts(mut r: ReadInsertUpdateMessage<Player>) {
    for m in r.read() {
        black_box(m.new.x);
    }
}

fn plugin() -> StdbPlugin<DbConnection, RemoteModule> {
    StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_uri(String::from("http://localhost:3000"))
        .with_database_name(String::from("bevy-stdb-simple"))
        .with_background_driver(DbConnection::run_threaded)
}

fn app(bound: Bound) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let plugin = plugin();

    match bound {
        Bound::Insert => {
            app.add_plugins(plugin.bind_insert::<PlayerTableAccessor>());
            app.add_systems(Update, read_inserts);
        }
        Bound::Mirror => {
            app.add_plugins(
                plugin
                    .bind_insert_update::<PlayerTableAccessor>()
                    .bind_delete::<PlayerTableAccessor>(),
            );
            app.add_systems(Update, (read_upserts, read_deletes));
        }
        Bound::All => {
            app.add_plugins(plugin.add_table::<PlayerTableAccessor>());
            app.add_systems(
                Update,
                (read_inserts, read_deletes, read_updates, read_upserts),
            );
        }
    }
    app
}

fn player(x: f32) -> Player {
    Player {
        identity: Identity::from_byte_array([0; 32]),
        online: true,
        x,
        y: 0.0,
    }
}

/// The event every change in a run shares.
///
/// `ReducerEvent` is `#[non_exhaustive]` and cannot be built outside the SDK, so this is a unit
/// variant: cloning it is free. The event clone that sharing one `Arc` avoids therefore does not
/// appear in these numbers at all, which understates the gain rather than inflating it -- a real
/// `Event::Reducer` carries the reducer's arguments and is cloned once per changed row.
fn shared_event() -> Event<Reducer> {
    Event::SubscribeApplied
}

/// The row events one frame delivers. `bind_insert` binds only `on_insert`, so nothing else
/// reaches the channel at all; the other two see the full mix.
fn row_events(bound: Bound, n: usize) -> Vec<TableChange<Player>> {
    let event = Arc::new(shared_event());
    (0..n)
        .map(|i| {
            let x = i as f32;
            let event = Some(Arc::clone(&event));
            match (bound, i % 4) {
                (Bound::Insert, _) | (_, 0) => TableChange::Insert {
                    event,
                    row: player(x),
                },
                (_, 3) => TableChange::Delete {
                    event,
                    row: player(x),
                },
                _ => TableChange::Update {
                    event,
                    old: player(x),
                    new: player(x + 1.0),
                },
            }
        })
        .collect()
}

fn sender(app: &App) -> Sender<TableChange<Player>> {
    app.world()
        .resource::<StdbChannels>()
        .sender::<TableChange<Player>>()
}

fn frame(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame");
    for bound in [Bound::Insert, Bound::Mirror, Bound::All] {
        for n in [100usize, 1_000, 10_000] {
            let mut app = app(bound);
            let tx = sender(&app);
            let events = row_events(bound, n);

            group.throughput(Throughput::Elements(n as u64));
            group.bench_with_input(BenchmarkId::new(bound.name(), n), &n, |b, _| {
                b.iter_batched(
                    || {
                        for change in &events {
                            tx.send(clone_change(change)).unwrap();
                        }
                    },
                    |()| app.update(),
                    BatchSize::PerIteration,
                );
            });
        }
    }
    group.finish();
}

fn produce(c: &mut Criterion) {
    let mut group = c.benchmark_group("produce");
    let n = 10_000usize;
    let mut app = app(Bound::All);
    let tx = sender(&app);
    let events = row_events(Bound::All, n);

    group.throughput(Throughput::Elements(n as u64));
    group.bench_function(BenchmarkId::new("add_table", n), |b| {
        b.iter_batched(
            // Drained outside the clock so the channel never grows.
            || app.update(),
            |()| {
                for change in &events {
                    tx.send(clone_change(change)).unwrap();
                }
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

#[derive(Message)]
struct Unused<const N: usize>;

/// Registers a hundred channels nothing ever sends on.
macro_rules! with_idle_channels {
    ($plugin:expr; $($n:literal)*) => { $plugin$(.add_channel_message::<Unused<$n>>())* };
}

fn idle(c: &mut Criterion) {
    let mut group = c.benchmark_group("idle");

    let mut one = app(Bound::All);
    group.bench_function("1_table", |b| b.iter(|| one.update()));

    let mut many = App::new();
    many.add_plugins(MinimalPlugins);
    many.add_plugins(
        with_idle_channels!(plugin().add_table::<PlayerTableAccessor>();
        0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24
        25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49
        50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65 66 67 68 69 70 71 72 73 74
        75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99),
    );
    many.add_systems(
        Update,
        (read_inserts, read_deletes, read_updates, read_upserts),
    );
    group.bench_function("1_table+100_channels", |b| b.iter(|| many.update()));

    group.finish();
}

/// `TableChange` is deliberately not `Clone` -- a change is delivered once -- so the bench
/// rebuilds each one the way a callback would.
fn clone_change(change: &TableChange<Player>) -> TableChange<Player> {
    match change {
        TableChange::Insert { event, row } => TableChange::Insert {
            event: event.clone(),
            row: row.clone(),
        },
        TableChange::Delete { event, row } => TableChange::Delete {
            event: event.clone(),
            row: row.clone(),
        },
        TableChange::Update { event, old, new } => TableChange::Update {
            event: event.clone(),
            old: old.clone(),
            new: new.clone(),
        },
    }
}

criterion_group!(benches, frame, produce, idle);
criterion_main!(benches);

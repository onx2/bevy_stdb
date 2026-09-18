//! Per-frame cost of delivering table changes to the readers an app binds.
//!
//! One iteration is one frame: the changes an SDK row callback would deliver for `n` row events,
//! pushed into the channel, then `app.update()`, which drains them and runs every reader system.
//! Everything before the send happens on the SDK's thread and needs a live connection, so the
//! sends stand in for it: one `TableChange` per row event, which is what one bound callback
//! produces however many capabilities want it.
//!
//! Bindings are matched to readers: an app binds the capabilities it reads and no more.
#[path = "../src/module_bindings/mod.rs"]
mod module_bindings;

use bevy::prelude::*;
use bevy_stdb::prelude::*;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
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

fn app(bound: Bound) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_uri(String::from("http://localhost:3000"))
        .with_database_name(String::from("bevy-stdb-simple"))
        .with_background_driver(DbConnection::run_threaded);

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

fn frame(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame");
    for bound in [Bound::Insert, Bound::Mirror, Bound::All] {
        for n in [100usize, 1_000, 10_000] {
            let mut app = app(bound);
            let tx = app
                .world()
                .resource::<StdbChannels>()
                .sender::<TableChange<Player>>();
            let events = row_events(bound, n);

            group.throughput(criterion::Throughput::Elements(n as u64));
            group.bench_with_input(BenchmarkId::new(bound.name(), n), &n, |b, _| {
                b.iter(|| {
                    for change in &events {
                        tx.send(clone_change(change)).unwrap();
                    }
                    app.update();
                });
            });
        }
    }
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

criterion_group!(benches, frame);
criterion_main!(benches);

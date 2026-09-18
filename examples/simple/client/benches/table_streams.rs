//! Per-frame cost of delivering table changes to the readers an app binds.
//!
//! One iteration is one frame: `n` changes pushed into the row type's channel, then `app.update()`
//! drains them and every registered reader system consumes them. That is the whole path the
//! design change touches -- everything before the channel send happens on the SDK's thread and
//! needs a live connection, so it is out of scope here.
//!
//! Written to compile against both the derived-message design and the view design, so the same
//! source can be run on either commit and the numbers compared.
#[path = "../src/module_bindings/mod.rs"]
mod module_bindings;

use bevy::prelude::*;
use bevy_stdb::prelude::*;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use module_bindings::{DbConnection, Player, PlayerTableAccessor, RemoteModule};
use spacetimedb_sdk::{Event, Identity};
use std::hint::black_box;
use std::sync::Arc;

/// Which readers the app binds, the axis the two designs differ on.
#[derive(Clone, Copy)]
enum Readers {
    /// One reader, the floor for either design.
    Insert,
    /// What a mirror binds: upserts plus deletes.
    Mirror,
    /// Every reader `add_table` allows.
    All,
}

impl Readers {
    fn name(self) -> &'static str {
        match self {
            Self::Insert => "insert only",
            Self::Mirror => "insert_update + delete",
            Self::All => "all four",
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

fn app(readers: Readers) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.add_plugins(
        StdbPlugin::<DbConnection, RemoteModule>::default()
            .with_uri(String::from("http://localhost:3000"))
            .with_database_name(String::from("bevy-stdb-simple"))
            .with_background_driver(DbConnection::run_threaded)
            .add_table::<PlayerTableAccessor>(),
    );
    match readers {
        Readers::Insert => {
            app.add_systems(Update, read_inserts);
        }
        Readers::Mirror => {
            app.add_systems(Update, (read_upserts, read_deletes));
        }
        Readers::All => {
            app.add_systems(Update, (read_inserts, read_deletes, read_updates, read_upserts));
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

/// A change mix in SDK callback order: inserts, updates, and deletes interleaved.
fn change(i: usize, event: &Arc<Event<module_bindings::Reducer>>) -> TableChange<Player> {
    let x = i as f32;
    match i % 4 {
        0 => TableChange::Insert {
            event: Some(Arc::clone(event)),
            row: player(x),
        },
        3 => TableChange::Delete {
            event: Some(Arc::clone(event)),
            row: player(x),
        },
        _ => TableChange::Update {
            event: Some(Arc::clone(event)),
            old: player(x),
            new: player(x + 1.0),
        },
    }
}

fn frame(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame");
    for readers in [Readers::Insert, Readers::Mirror, Readers::All] {
        for changes in [100usize, 1_000, 10_000] {
            let mut app = app(readers);
            let tx = app
                .world()
                .resource::<StdbChannels>()
                .sender::<TableChange<Player>>();
            let event = Arc::new(Event::SubscribeApplied);

            group.throughput(criterion::Throughput::Elements(changes as u64));
            group.bench_with_input(
                BenchmarkId::new(readers.name(), changes),
                &changes,
                |b, &changes| {
                    b.iter(|| {
                        for i in 0..changes {
                            tx.send(change(i, &event)).unwrap();
                        }
                        app.update();
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, frame);
criterion_main!(benches);

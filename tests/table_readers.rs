//! The table readers as views over the one `TableChange` stream a row type has.
//!
//! No test here connects. Each pushes changes into the channel a bound SDK row callback would
//! send on, which is everything downstream of the SDK's thread.
// The generated bindings select their native API by target, the SDK by feature, so they cannot
// compile natively once `--all-features` turns `browser` on.
#![cfg(not(feature = "browser"))]
mod support;

use bevy_app::App;
use bevy_ecs::system::RunSystemOnce;
use bevy_stdb::prelude::*;
use spacetimedb_sdk::Event;
use std::sync::Arc;
use support::{
    app_with, event,
    module_bindings::{Player, PlayerTableAccessor},
    player, sender,
};

fn app() -> App {
    app_with(|p| p.add_table::<PlayerTableAccessor>())
}

#[test]
fn one_change_reaches_every_bound_reader() {
    let mut app = app();
    let tx = sender(&app);

    tx.send(TableChange::Insert {
        event: event(),
        row: player(1.0),
    })
    .unwrap();
    tx.send(TableChange::Update {
        event: event(),
        old: player(1.0),
        new: player(2.0),
    })
    .unwrap();
    tx.send(TableChange::Delete {
        event: event(),
        row: player(2.0),
    })
    .unwrap();
    app.update();

    let w = app.world_mut();
    let unified = w
        .run_system_once(|mut r: ReadTableChangeMessage<Player>| r.read().count())
        .unwrap();
    let inserts = w
        .run_system_once(|mut r: ReadInsertMessage<Player>| r.read().count())
        .unwrap();
    let updates = w
        .run_system_once(|mut r: ReadUpdateMessage<Player>| r.read().count())
        .unwrap();
    let deletes = w
        .run_system_once(|mut r: ReadDeleteMessage<Player>| r.read().count())
        .unwrap();
    let upserts = w
        .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| r.read().count())
        .unwrap();

    assert_eq!(
        (unified, inserts, updates, deletes, upserts),
        (3, 1, 1, 1, 2)
    );
}

#[test]
fn a_reader_yields_the_values_of_the_change_it_filtered() {
    // Counting alone would pass if a reader yielded the wrong row or dropped `old`.
    let mut app = app();
    sender(&app)
        .send(TableChange::Update {
            event: event(),
            old: player(1.0),
            new: player(2.0),
        })
        .unwrap();
    app.update();

    let w = app.world_mut();
    let updates = w
        .run_system_once(|mut r: ReadUpdateMessage<Player>| {
            r.read().map(|m| (m.old.x, m.new.x)).collect::<Vec<_>>()
        })
        .unwrap();
    let upserts = w
        .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| {
            r.read()
                .map(|m| (m.old.map(|o| o.x), m.new.x))
                .collect::<Vec<_>>()
        })
        .unwrap();

    assert_eq!(updates, vec![(1.0, 2.0)]);
    assert_eq!(upserts, vec![(Some(1.0), 2.0)]);
}

#[test]
fn an_insert_reaches_insert_update_with_no_old_row() {
    let mut app = app();
    sender(&app)
        .send(TableChange::Insert {
            event: event(),
            row: player(7.0),
        })
        .unwrap();
    app.update();

    let upserts = app
        .world_mut()
        .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| {
            r.read()
                .map(|m| (m.old.is_some(), m.new.x))
                .collect::<Vec<_>>()
        })
        .unwrap();

    assert_eq!(upserts, vec![(false, 7.0)]);
}

#[test]
fn a_delete_reaches_no_insert_or_insert_update_stream() {
    let mut app = app();
    sender(&app)
        .send(TableChange::Delete {
            event: event(),
            row: player(1.0),
        })
        .unwrap();
    app.update();

    let w = app.world_mut();
    let inserts = w
        .run_system_once(|mut r: ReadInsertMessage<Player>| r.read().count())
        .unwrap();
    let upserts = w
        .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| r.read().count())
        .unwrap();

    assert_eq!((inserts, upserts), (0, 0));
}

#[test]
fn the_unified_stream_keeps_callback_order() {
    let mut app = app();
    let tx = sender(&app);

    for x in 0..5 {
        let _ = tx.send(if x % 2 == 0 {
            TableChange::Insert {
                event: event(),
                row: player(x as f32),
            }
        } else {
            TableChange::Delete {
                event: event(),
                row: player(x as f32),
            }
        });
    }
    app.update();

    let seen = app
        .world_mut()
        .run_system_once(|mut r: ReadTableChangeMessage<Player>| {
            r.read()
                .map(|c| match c {
                    TableChange::Insert { row, .. } => ('i', row.x),
                    TableChange::Delete { row, .. } => ('d', row.x),
                    TableChange::Update { new, .. } => ('u', new.x),
                })
                .collect::<Vec<_>>()
        })
        .unwrap();

    assert_eq!(
        seen,
        vec![('i', 0.0), ('d', 1.0), ('i', 2.0), ('d', 3.0), ('i', 4.0)]
    );
}

#[test]
fn a_change_is_read_exactly_once_across_frames() {
    // Bevy keeps messages for two frames. Each reader holds its own cursor, so a second
    // frame must not re-read what the first already did.
    let mut app = app();
    sender(&app)
        .send(TableChange::Insert {
            event: event(),
            row: player(1.0),
        })
        .unwrap();
    app.update();
    app.update();

    let inserts = app
        .world_mut()
        .run_system_once(|mut r: ReadInsertMessage<Player>| r.read().count())
        .unwrap();

    assert_eq!(inserts, 1);
}

#[test]
fn readers_borrow_one_event_rather_than_cloning_it() {
    let mut app = app();
    let event = Arc::new(Event::SubscribeApplied);

    sender(&app)
        .send(TableChange::Insert {
            event: Some(Arc::clone(&event)),
            row: player(1.0),
        })
        .unwrap();
    app.update();

    // Held here and by the one `TableChange`. Reading it through every table reader adds
    // nothing: the readers borrow out of that single stored change.
    let w = app.world_mut();
    let _ = w
        .run_system_once(|mut r: ReadInsertMessage<Player>| r.read().count())
        .unwrap();
    let _ = w
        .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| r.read().count())
        .unwrap();

    assert_eq!(Arc::strong_count(&event), 2);
}

#[test]
fn a_reader_borrows_the_row_out_of_the_stream() {
    // The point of the views: the row a reader yields is the one the change holds, not a
    // copy of it.
    let mut app = app();
    sender(&app)
        .send(TableChange::Insert {
            event: event(),
            row: player(3.0),
        })
        .unwrap();
    app.update();

    let same = app
        .world_mut()
        .run_system_once(
            |mut changes: ReadTableChangeMessage<Player>,
             mut inserts: ReadInsertMessage<Player>| {
                let stored = changes
                    .read()
                    .map(|c| match c {
                        TableChange::Insert { row, .. } => std::ptr::from_ref(row),
                        _ => unreachable!(),
                    })
                    .collect::<Vec<_>>();
                let borrowed = inserts
                    .read()
                    .map(|i| std::ptr::from_ref(i.row))
                    .collect::<Vec<_>>();
                stored == borrowed
            },
        )
        .unwrap();

    assert!(same);
}

#[test]
fn a_borrowed_row_still_clones_into_an_owned_one() {
    // The migration guide promises `msg.row.clone()` keeps working; method resolution
    // reaches `Player`'s own `Clone` through the borrow rather than cloning the reference.
    let mut app = app();
    sender(&app)
        .send(TableChange::Insert {
            event: event(),
            row: player(5.0),
        })
        .unwrap();
    app.update();

    let owned = app
        .world_mut()
        .run_system_once(|mut r: ReadInsertMessage<Player>| {
            r.read().map(|m| m.row.clone()).collect::<Vec<Player>>()
        })
        .unwrap();

    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].x, 5.0);
}

#[test]
#[should_panic(expected = "no `Update` callback is bound")]
fn reading_a_stream_the_table_never_bound_panics() {
    // `add_table_without_pk` binds insert and delete only, so an update reader would
    // otherwise read as a table that never updates.
    let mut app = app_with(|p| p.add_table_without_pk::<PlayerTableAccessor>());

    let _ = app
        .world_mut()
        .run_system_once(|mut r: ReadUpdateMessage<Player>| r.read().count());
}

#[test]
fn a_row_type_without_events_yields_none_to_every_reader() {
    let mut app = app_with(|p| {
        p.add_table::<PlayerTableAccessor>()
            .without_event::<PlayerTableAccessor>()
    });
    sender(&app)
        .send(TableChange::Insert {
            event: None,
            row: player(1.0),
        })
        .unwrap();
    app.update();

    let w = app.world_mut();
    let inserts = w
        .run_system_once(|mut r: ReadInsertMessage<Player>| {
            r.read().map(|m| m.event.is_some()).collect::<Vec<_>>()
        })
        .unwrap();
    let upserts = w
        .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| {
            r.read().map(|m| m.event.is_some()).collect::<Vec<_>>()
        })
        .unwrap();

    assert_eq!((inserts, upserts), (vec![false], vec![false]));
}

#[test]
fn inserts_and_updates_bound_separately_feed_the_insert_update_reader() {
    // There is no insert-update callback to bind, only a reader that needs the other two.
    let mut app = app_with(|p| {
        p.bind_insert::<PlayerTableAccessor>()
            .bind_update::<PlayerTableAccessor>()
    });
    sender(&app)
        .send(TableChange::Insert {
            event: event(),
            row: player(1.0),
        })
        .unwrap();
    app.update();

    let upserts = app
        .world_mut()
        .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| r.read().count())
        .unwrap();

    assert_eq!(upserts, 1);
}

#[test]
#[should_panic(expected = "no `Update` callback is bound")]
fn the_insert_update_reader_needs_updates_as_well_as_inserts() {
    let mut app = app_with(|p| p.bind_insert::<PlayerTableAccessor>());

    let _ = app
        .world_mut()
        .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| r.read().count());
}

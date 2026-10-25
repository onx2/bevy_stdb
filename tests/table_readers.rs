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
use support::{
    app_with, ctx,
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
        ctx: ctx(),
        row: player(1.0),
    })
    .unwrap();
    tx.send(TableChange::Update {
        ctx: ctx(),
        old: player(1.0),
        new: player(2.0),
    })
    .unwrap();
    tx.send(TableChange::Delete {
        ctx: ctx(),
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
            ctx: ctx(),
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
            ctx: ctx(),
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
            ctx: ctx(),
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
                ctx: ctx(),
                row: player(x as f32),
            }
        } else {
            TableChange::Delete {
                ctx: ctx(),
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
            ctx: ctx(),
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
fn readers_borrow_the_one_ctx_the_change_holds() {
    let mut app = app_with(|p| p.add_table_with_ctx::<PlayerTableAccessor>());
    sender(&app)
        .send(TableChange::Insert {
            ctx: ctx(),
            row: player(1.0),
        })
        .unwrap();
    app.update();

    let same = app
        .world_mut()
        .run_system_once(
            |mut changes: ReadTableChangeMessage<Player>,
             mut inserts: ReadInsertMessage<Player>,
             mut upserts: ReadInsertUpdateMessage<Player>| {
                let stored = std::ptr::from_ref(changes.read().next().unwrap().ctx().event);
                let inserted = std::ptr::from_ref(inserts.read().next().unwrap().ctx().event);
                let upserted = std::ptr::from_ref(upserts.read().next().unwrap().ctx().event);
                stored == inserted && stored == upserted
            },
        )
        .unwrap();

    assert!(same);
}

#[test]
fn a_reader_borrows_the_row_out_of_the_stream() {
    // A reader yields the row the change holds, not a copy of it.
    let mut app = app();
    sender(&app)
        .send(TableChange::Insert {
            ctx: ctx(),
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
    // `msg.row.clone()` gives an owned row: method resolution finds `Player`'s `Clone` through
    // the reference.
    let mut app = app();
    sender(&app)
        .send(TableChange::Insert {
            ctx: ctx(),
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
fn an_opted_in_ctx_reaches_every_reader() {
    let mut app = app_with(|p| p.add_table_with_ctx::<PlayerTableAccessor>());
    sender(&app)
        .send(TableChange::Insert {
            ctx: ctx(),
            row: player(1.0),
        })
        .unwrap();
    app.update();

    let w = app.world_mut();
    let changes = w
        .run_system_once(|mut r: ReadTableChangeMessage<Player>| {
            r.read()
                .map(|c| matches!(c.ctx().event, Event::SubscribeApplied))
                .collect::<Vec<_>>()
        })
        .unwrap();
    let inserts = w
        .run_system_once(|mut r: ReadInsertMessage<Player>| {
            r.read()
                .map(|m| matches!(m.ctx().event, Event::SubscribeApplied))
                .collect::<Vec<_>>()
        })
        .unwrap();

    assert_eq!((changes, inserts), (vec![true], vec![true]));
}

/// An app whose row type was not bound with `TableCapability::ctx`, holding one change without a
/// context, as its callbacks send.
fn app_with_a_change_without_ctx() -> App {
    let mut app = app();
    sender(&app)
        .send(TableChange::Insert {
            ctx: None.into(),
            row: player(1.0),
        })
        .unwrap();
    app.update();
    app
}

#[test]
fn try_ctx_is_none_for_a_row_type_that_did_not_opt_in() {
    let mut app = app_with_a_change_without_ctx();

    let w = app.world_mut();
    let changes = w
        .run_system_once(|mut r: ReadTableChangeMessage<Player>| {
            r.read().map(|c| c.try_ctx().is_none()).collect::<Vec<_>>()
        })
        .unwrap();
    let upserts = w
        .run_system_once(|mut r: ReadInsertUpdateMessage<Player>| {
            r.read().map(|m| m.try_ctx().is_none()).collect::<Vec<_>>()
        })
        .unwrap();

    assert_eq!((changes, upserts), (vec![true], vec![true]));
}

#[test]
#[should_panic(expected = "bind `TableCapability::ctx()`")]
fn ctx_panics_naming_the_opt_in_for_a_row_type_that_did_not_opt_in() {
    // Silently reading `None` would look like a table whose changes have no cause.
    let mut app = app_with_a_change_without_ctx();

    let _ = app
        .world_mut()
        .run_system_once(|mut r: ReadInsertMessage<Player>| {
            for m in r.read() {
                let _ = m.ctx();
            }
        });
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
            ctx: ctx(),
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

/// Sends an insert of `x`, an update from `x`, a delete, and a second insert: two inserts, one
/// update, and one delete, in that order.
fn app_with_every_kind_of_change() -> App {
    let mut app = app();
    let tx = sender(&app);
    for change in [
        TableChange::Insert {
            ctx: ctx(),
            row: player(1.0),
        },
        TableChange::Update {
            ctx: ctx(),
            old: player(1.0),
            new: player(2.0),
        },
        TableChange::Delete {
            ctx: ctx(),
            row: player(2.0),
        },
        TableChange::Insert {
            ctx: ctx(),
            row: player(3.0),
        },
    ] {
        tx.send(change).unwrap();
    }
    app.update();
    app
}

#[test]
fn len_and_is_empty_count_their_kind_without_reading_it() {
    let mut app = app_with_every_kind_of_change();

    let counts = app
        .world_mut()
        .run_system_once(
            |mut inserts: ReadInsertMessage<Player>,
             deletes: ReadDeleteMessage<Player>,
             upserts: ReadInsertUpdateMessage<Player>| {
                let before = (
                    inserts.len(),
                    deletes.len(),
                    upserts.len(),
                    inserts.is_empty(),
                );
                let read = inserts.read().count();
                (before, read, inserts.len(), inserts.is_empty())
            },
        )
        .unwrap();

    assert_eq!(counts, ((2, 1, 3, false), 2, 0, true));
}

#[test]
fn clear_marks_every_unread_change_as_read() {
    let mut app = app_with_every_kind_of_change();

    let after = app
        .world_mut()
        .run_system_once(|mut inserts: ReadInsertMessage<Player>| {
            inserts.clear();
            (inserts.is_empty(), inserts.read().count())
        })
        .unwrap();

    assert_eq!(after, (true, 0));
}

#[test]
fn read_with_id_yields_the_ids_of_its_kind() {
    let mut app = app_with_every_kind_of_change();

    let (all, inserts) = app
        .world_mut()
        .run_system_once(
            |mut changes: ReadTableChangeMessage<Player>,
             mut inserts: ReadInsertMessage<Player>| {
                let all = changes.read_with_id().map(|(_, id)| id).collect::<Vec<_>>();
                let inserts = inserts
                    .read_with_id()
                    .map(|(insert, id)| (insert.row.x, id))
                    .collect::<Vec<_>>();
                (all, inserts)
            },
        )
        .unwrap();

    assert_eq!(inserts, vec![(1.0, all[0]), (3.0, all[3])]);
}

#[test]
#[should_panic(expected = "no `Update` callback is bound")]
fn len_panics_like_read_for_a_kind_the_table_never_bound() {
    let mut app = app_with(|p| p.add_table_without_pk::<PlayerTableAccessor>());

    let _ = app
        .world_mut()
        .run_system_once(|r: ReadUpdateMessage<Player>| r.len());
}

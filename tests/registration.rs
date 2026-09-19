//! Whatever is bound, and in whatever order, the row type's one channel exists afterwards.
// The generated bindings select their native API by target, the SDK by feature, so they cannot
// compile natively once `--all-features` turns `browser` on.
#![cfg(not(feature = "browser"))]
mod support;

use bevy_app::App;
use support::{app_with, module_bindings::PlayerTableAccessor, sender};

/// The channel every bound capability sends into must exist before a connection binds it,
/// whatever order the capabilities were registered in. `channel_sender` panics otherwise,
/// and it runs when the connection becomes active rather than at startup.
fn assert_change_channel_registered(app: &App) {
    let _ = sender(app);
}

#[test]
fn add_table_registers_the_change_channel() {
    assert_change_channel_registered(&app_with(|p| p.add_table::<PlayerTableAccessor>()));
}

#[test]
fn insert_update_before_insert_registers_the_change_channel() {
    // Every capability registers the shared channel, so claiming `InsertUpdate` first must
    // not leave a later `Insert` believing someone else already did it -- nor register it
    // twice, which `register_channel` panics on.
    assert_change_channel_registered(&app_with(|p| {
        p.bind_insert_update::<PlayerTableAccessor>()
            .bind_insert::<PlayerTableAccessor>()
    }));
}

#[test]
fn every_add_method_registers_the_change_channel() {
    assert_change_channel_registered(&app_with(|p| {
        p.add_table_without_pk::<PlayerTableAccessor>()
    }));
    assert_change_channel_registered(&app_with(|p| p.add_view::<PlayerTableAccessor>()));
    assert_change_channel_registered(&app_with(|p| p.add_event_table::<PlayerTableAccessor>()));
}

#[test]
fn opting_out_of_events_still_registers_everything_else() {
    // `without_event` only changes what a bound callback puts in the message; it must not
    // disturb registration, and it is order-independent.
    assert_change_channel_registered(&app_with(|p| {
        p.without_event::<PlayerTableAccessor>()
            .add_table::<PlayerTableAccessor>()
            .without_event::<PlayerTableAccessor>()
    }));
}

#[test]
fn binding_what_is_already_bound_is_harmless() {
    // `add_table` already binds inserts. Asking again -- from another plugin's setup, say --
    // must neither panic nor register the channel a second time.
    assert_change_channel_registered(&app_with(|p| {
        p.add_table::<PlayerTableAccessor>()
            .bind_insert::<PlayerTableAccessor>()
            .bind_insert_update::<PlayerTableAccessor>()
            .add_table::<PlayerTableAccessor>()
    }));
}

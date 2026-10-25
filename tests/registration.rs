//! Whatever is bound, and in whatever order, the row type's one channel exists afterwards.
// The generated bindings select their native API by target, the SDK by feature, so they cannot
// compile natively once `--all-features` turns `browser` on.
#![cfg(not(feature = "browser"))]
mod support;

use bevy_app::App;
use bevy_stdb::prelude::TableCapability;
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
    // Whichever capability comes first registers the shared channel, exactly once:
    // `register_channel` panics on a second registration.
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
fn binding_ctx_still_registers_everything_else() {
    // `TableCapability::ctx` binds no callback of its own; it must not disturb registration, and
    // it is order-independent.
    assert_change_channel_registered(&app_with(|p| {
        p.bind::<PlayerTableAccessor>([TableCapability::ctx()])
            .add_table::<PlayerTableAccessor>()
            .add_table_with_ctx::<PlayerTableAccessor>()
    }));
}

#[test]
fn binding_what_is_already_bound_is_harmless() {
    // `add_table` already binds inserts. Binding them again, from another plugin's setup for
    // example, must neither panic nor register the channel twice.
    assert_change_channel_registered(&app_with(|p| {
        p.add_table::<PlayerTableAccessor>()
            .bind_insert::<PlayerTableAccessor>()
            .bind_insert_update::<PlayerTableAccessor>()
            .add_table::<PlayerTableAccessor>()
    }));
}

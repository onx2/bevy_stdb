//! A frame-driven connection runs its SDK callbacks inside the frame, so what they deliver must
//! be readable in that same frame.
// The generated bindings select their native API by target, the SDK by feature, so they cannot
// compile natively once `--all-features` turns `browser` on.
#![cfg(not(feature = "browser"))]
mod support;

use bevy_app::{PreUpdate, Update};
use bevy_ecs::prelude::*;
use bevy_stdb::prelude::*;
use support::module_bindings::DbConnection;

#[derive(Message)]
struct Delivered;

#[derive(Resource, Default)]
struct Seen(usize);

#[test]
fn what_the_driver_delivers_is_read_in_the_same_frame() {
    let mut app = bevy_app::App::new();
    app.add_plugins(
        support::target()
            .with_frame_driver(DbConnection::frame_tick)
            .add_channel_message::<Delivered>(),
    );
    app.init_resource::<Seen>();
    // Stands in for an SDK callback firing inside `frame_tick`: it sends on a channel from
    // the set the driver runs in.
    app.add_systems(
        PreUpdate,
        (|channels: Res<StdbChannels>| {
            let _ = channels.sender::<Delivered>().send(Delivered);
        })
        .in_set(StdbSet::Drive),
    );
    app.add_systems(
        Update,
        |mut delivered: MessageReader<Delivered>, mut seen: ResMut<Seen>| {
            seen.0 += delivered.read().count();
        },
    );

    app.update();

    assert_eq!(app.world().resource::<Seen>().0, 1);
}

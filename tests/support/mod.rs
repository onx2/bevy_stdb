//! Shared fixtures for the integration tests.
//!
//! The bindings are the example's `spacetime generate` output, included by path: they depend on
//! nothing but `spacetimedb-sdk`, and one copy cannot drift from the example it documents.
#![allow(dead_code)]

// Skipped so `cargo fmt` leaves the committed `spacetime generate` output alone.
#[rustfmt::skip]
#[path = "../../examples/simple/client/src/module_bindings/mod.rs"]
pub mod module_bindings;

use bevy_app::App;
use bevy_stdb::prelude::*;
use module_bindings::{DbConnection, Player, RemoteModule};
use spacetimedb_sdk::{Event, Identity};
use std::sync::Arc;

pub type Plugin = StdbPlugin<DbConnection, RemoteModule>;

/// The required target settings, with no driver chosen yet.
pub fn target() -> Plugin {
    Plugin::default()
        .with_uri("http://localhost:3000")
        .with_database_name("bevy-stdb-simple")
}

/// A background-driven plugin with nothing bound. Only the `live` tests ever dial the target.
pub fn plugin() -> Plugin {
    target().with_background_driver(DbConnection::run_threaded)
}

/// An app running `configure(plugin())`.
pub fn app_with(configure: impl FnOnce(Plugin) -> Plugin) -> App {
    let mut app = App::new();
    app.add_plugins(configure(plugin()));
    app
}

pub fn player(x: f32) -> Player {
    Player {
        identity: Identity::from_byte_array([0; 32]),
        online: true,
        x,
        y: 0.0,
    }
}

pub fn event() -> MaybeRowEvent<Player> {
    Some(Arc::new(Event::SubscribeApplied))
}

/// The sender a bound SDK row callback would hold.
pub fn sender(app: &App) -> Sender<TableChange<Player>> {
    app.world()
        .resource::<StdbChannels>()
        .sender::<TableChange<Player>>()
}

# bevy_stdb

A [Bevy](https://bevy.org/) integration for [SpacetimeDB](https://spacetimedb.com).
[![crates.io](https://img.shields.io/crates/v/bevy_stdb)](https://crates.io/crates/bevy_stdb)
[![docs.rs](https://docs.rs/bevy_stdb/badge.svg)](https://docs.rs/bevy_stdb)

![Useless AI generated image that kind of looks cool](https://github.com/user-attachments/assets/b6cf0408-0c0d-4997-bf9c-e2e0989ab5f3)
_Please enjoy this useless AI generated image based on the README contents of this repo._



## Overview

`bevy_stdb` adapts SpacetimeDB's connection and callback model into Bevy-style resources, systems, plugins, and messages. Its built around a few core ideas:

- Configure everything through `StdbPlugin`
- Expose the active live connection as a Bevy resource via `StdbConnection`
- Forward table callbacks into Bevy messages with `TableRegistrar`
- Store subscription intent independently from the live connection with `StdbSubscriptions`
- Optionally retry failed connections with `StdbReconnectOptions`

The library is organized around connection-scoped lifecycle concerns:

- **connection lifecycle**: establish the initial connection, expose the active connection resource, and track connection state
- **table lifecycle**: initialize table message channels once and re-bind table callbacks whenever a connection becomes active
- **subscription lifecycle**: store desired subscription intent and re-apply queued subscriptions when connected
- **reconnect lifecycle**: optionally retry connection attempts after disconnects using configurable backoff

## Features

- **Builder-style setup** via `StdbPlugin`
- **Connection resource** access through `StdbConnection`
- **Table event bridging** into normal Bevy `Message`s
- **Managed subscription intent** through `StdbSubscriptions`
- **Optional reconnect support** through `StdbReconnectOptions`

## Example

```rust
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use crate::module_bindings::{DbConnection, RemoteModule};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum MySubKey {
    PlayerInfo,
}

pub type StdbConn = StdbConnection<DbConnection>;
pub type StdbSubs = StdbSubscriptions<MySubKey, RemoteModule>;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_module_name("my_module")
                .with_uri("http://localhost:3000")
                .with_tables(|reg, db| {
                    reg.table(&db.player_info());
                    reg.table_without_pk(&db.nearby_monsters());
                })
                .with_subscriptions::<MySubKey>(|subs| {
                    // This is a great place to add "global" subscriptions,
                    // but you can subscribe from any system too.
                    subs.subscribe_query(MySubKey::PlayerInfo, |q| q.from.player_info());
                })
                .with_reconnect(StdbReconnectOptions::default())
                .with_background_driver(DbConnection::run_threaded),
        )
        .add_systems(Update, on_player_info_insert)
        .run();
}

fn on_player_info_insert(mut msgs: ReadInsertMessage<PlayerInfo>) {
    for msg in msgs.read() {
        info!("player inserted: {:?}", msg.row);
    }
}
```

## Connection driving

`bevy_stdb` supports two connection-driving modes:

- `with_background_driver(...)`: start SpacetimeDB's background processing for the active connection
- `with_frame_driver(...)`: drive SpacetimeDB from the Bevy schedule each frame

Exactly one driver must be configured. These modes are mutually exclusive, and in most applications you'll want `with_background_driver(...)`.

If WASM support is needed, you can enable the `browser` feature flag in both this crate and your `spacetimedb-sdk` crate using a target cfg:

```toml
# Enable browser support for wasm builds.
# Replace `*` with the versions you are using.
[target.wasm32-unknown-unknown.dependencies]
spacetimedb-sdk = { version = "*", features = ["browser"] }
bevy_stdb = { version = "*", features = ["browser"] }
```

> I recommend checking out the [bevy_cli 2d template](https://github.com/TheBevyFlock/bevy_new_2d/) for a good starter example using WASM + native with nice Bevy features configured.

### Native background driving

On native targets, the typical choice is `run_threaded`:

```rust
fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_module_name("my_module")
                .with_uri("http://localhost:3000")
                .with_background_driver(DbConnection::run_threaded),
        )
        .run();
}
```

### Browser / wasm background driving (async)

On browser targets, use the generated background task helper instead:

```rust
fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_module_name("my_module")
                .with_uri("http://localhost:3000")
                .with_background_driver(DbConnection::run_background_task),
        )
        .run();
}
```

If you target both native and browser, I recommend selecting the background driver with `cfg`:

```rust
fn main() {
    let mut plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_module_name("my_module")
        .with_uri("http://localhost:3000");
    
    #[cfg(target_arch = "wasm32")]
    {
        plugin = plugin.with_background_driver(DbConnection::run_background_task);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        plugin = plugin.with_background_driver(DbConnection::run_threaded);
    }

    App::new().add_plugins(DefaultPlugins).add_plugins(plugin).run();
}
```

### Bevy frame-tick driving

Use `frame_tick` when you want Bevy to drive connection progress from Bevy each frame. Internally, `bevy_stdb` runs this driver from `PreUpdate`:

```rust
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use crate::module_bindings::{DbConnection, RemoteModule};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_module_name("my_module")
                .with_uri("http://localhost:3000")
                .with_frame_driver(DbConnection::frame_tick),
        )
        .run();
}
```

## Table registration

Use `StdbPlugin::with_tables` to register all table callbacks in one place.

The closure receives both a `TableRegistrar` and the current database view. Use the `db` argument to select tables, views, and event streams, then register them through `reg`.

Typical forms are:

```rust
.with_tables(|reg, db| {
    reg.table(&db.player_info());
    reg.table_without_pk(&db.nearby_monsters());
    reg.event_table(&db.some_event_stream());
    reg.view(&db.some_view());
})
```

`with_tables` is intended to be called once during plugin setup. These registrations are replayed during initialization to install the required Bevy message channels and again whenever the connection enters the connected state to bind callbacks for the current live database view.

## Messages

Depending on the table shape, `bevy_stdb` forwards updates into Bevy messages such as:

- `InsertMessage<T>`
- `DeleteMessage<T>`
- `UpdateMessage<T>`
- `InsertUpdateMessage<T>`

This lets normal Bevy systems react to database changes using message readers.

## Subscriptions

`StdbSubscriptions` stores desired subscription intent separately from the live connection.

That means you can:

- declare global (or any other) subscriptions during plugin setup with `with_subscriptions`
- queue additional subscriptions later from normal Bevy systems
- automatically re-apply queued subscription intent after reconnect

Like the other `StdbPlugin` configuration methods, `with_subscriptions` is intended to be configured once during plugin setup.

Subscriptions are keyed, so the application can refer to them using its own domain-specific identifiers, allowing you to also unsubscribe as needed later.

## Reconnects

Reconnect behavior is opt-in.

Use `StdbPlugin::with_reconnect` with `StdbReconnectOptions` to enable retry behavior after disconnects. Like the other `StdbPlugin` configuration methods, it is intended to be configured once during plugin setup. When reconnect succeeds:

- the live `StdbConnection` resource is replaced
- table callbacks are re-bound
- queued subscriptions are re-applied

## Type Aliases

It is useful to define some type aliases of your own. I suggest doing something like this:

```rust
#[derive(Clone, Eq, Hash, PartialEq, Debug)]
pub enum SubKeys {
    PlayerInfo,
    TimeOfDay,
}

pub type StdbConn = StdbConnection<DbConnection>;
pub type StdbSubs = StdbSubscriptions<SubKeys, RemoteModule>;

// Or a more constrained version for typical use cases:
// pub type StdbConn<'w> = Res<'w, StdbConnection<DbConnection>>;
// pub type StdbSubs<'w> = ResMut<'w, StdbSubscriptions<SubKeys, RemoteModule>>;

// Usage example
fn example_system(conn: Res<StdbConn>, mut subs: ResMut<StdbSubs>) {
    let my_table = conn.db().player_info().id().find(&1);
    subs.subscribe_query(SubKeys::TimeOfDay, |q| q.from.world_clock());
}
```

## Compatibility

| bevy_stdb | bevy   | spacetimedb_sdk |
| --------- | ------ | --------------- |
| 0.1 - 0.2 | 0.18   | 2.0             |
| 0.3 - 0.4 | 0.18   | 2.1             |

## Notes

This crate focuses on table-driven client workflows. Reducer and procedure access still exist through the active `StdbConnection`, but the primary Bevy-facing event flow is table/message based.

The project was heavily inspired by [`bevy_spacetimedb`](https://docs.rs/bevy_spacetimedb/), but takes a different approach to plugin structure, connection lifecycle handling, reconnect behavior, and subscription restoration.

# bevy_stdb

A [Bevy](https://bevy.org/) integration for [SpacetimeDB](https://spacetimedb.com).

[![crates.io](https://img.shields.io/crates/v/bevy_stdb)](https://crates.io/crates/bevy_stdb)
![Dependabot](https://img.shields.io/badge/dependabot-enabled-brightgreen.svg)
[![docs.rs](https://docs.rs/bevy_stdb/badge.svg)](https://docs.rs/bevy_stdb)
[![CI](https://github.com/onx2/bevy_stdb/actions/workflows/ci.yml/badge.svg)](https://github.com/onx2/bevy_stdb/actions/workflows/ci.yml)
[![CodeQL](https://github.com/onx2/bevy_stdb/actions/workflows/github-code-scanning/codeql/badge.svg)](https://github.com/onx2/bevy_stdb/actions/workflows/github-code-scanning/codeql)

![Useless AI generated image that kind of looks cool](https://github.com/user-attachments/assets/b6cf0408-0c0d-4997-bf9c-e2e0989ab5f3)
_Please enjoy this useless AI generated image based on the README contents of this repo._



## Overview

`bevy_stdb` adapts SpacetimeDB's connection and callback model into Bevy-style resources, systems, plugins, and messages. Its built around a few core ideas:

- Configure everything through `StdbPlugin`
- Expose the active live connection as a Bevy resource via `StdbConnection`
- Forward SpacetimeDB table callbacks into Bevy `Message`s
- Store subscription intent independently from the live connection with `StdbSubscriptions`
- Optionally retry failed connections with `StdbReconnectOptions`

The library is organized around connection-scoped lifecycle concerns:

- **connection lifecycle**: establish the initial connection eagerly or on demand, expose the active connection resource, and track connection state
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
use crate::module_bindings::{DbConnection, PlayerInfo, RemoteModule};

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
                .add_table::<PlayerInfo>(|reg, db| reg.bind(db.player_info()))
                .with_subscriptions::<MySubKey>()
                .with_reconnect(StdbReconnectOptions::default())
                .with_background_driver(DbConnection::run_threaded),
        )
        .add_systems(Update, (subscribe_on_connect, on_player_info_insert))
        .run();
}

fn subscribe_on_connect(
    mut connected: ReadStdbConnectedMessage,
    mut subs: ResMut<StdbSubs>,
) {
    if connected.read().next().is_some() {
        subs.subscribe_query(MySubKey::PlayerInfo, |q| q.from.player_info());
    }
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
    let stdb_plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_module_name("my_module")
        .with_uri("http://localhost:3000")
        .with_background_driver(DbConnection::run_threaded);
}
```

### Browser / wasm background driving (async)

On browser targets, use the generated background task helper instead:

```rust
fn main() {
    let stdb_plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_module_name("my_module")
        .with_uri("http://localhost:3000")
        .with_background_driver(DbConnection::run_background_task)
}
```

If you target both native and browser, I recommend selecting the background driver with `cfg`:

```rust
fn main() {
    let mut plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_module_name("my_module")
        .with_uri("http://localhost:3000");

    #[cfg(target_arch = "wasm32")]
    let driver = DbConnection::run_background_task;
    #[cfg(not(target_arch = "wasm32"))]
    let driver = DbConnection::run_threaded;
    
    stdb_plugin = stdb_plugin.with_background_driver(driver);
}
```

### Bevy frame-tick driving

Use `frame_tick` when you want Bevy to drive connection progress from Bevy each frame. Internally, `bevy_stdb` runs this driver from `PreUpdate`:

```rust
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use crate::module_bindings::{DbConnection, RemoteModule};

fn main() {
    let stdb_plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_module_name("my_module")
        .with_uri("http://localhost:3000")
        .with_frame_driver(DbConnection::frame_tick);
}
```

## Table registration

Use the `StdbPlugin` builder methods to register table bindings during app setup.

Each method eagerly registers the Bevy message channels for the row type you specify and stores a deferred binding callback that runs whenever a connection becomes active.

```rust
.add_table::<PlayerInfo>(|reg, db| reg.bind(db.player_info()))
.add_table_without_pk::<WorldClock>(|reg, db| reg.bind(db.world_clock()))
.add_event_table::<DamageEvent>(|reg, db| reg.bind(db.damage_events()))
.add_view::<NearbyMonster>(|reg, db| reg.bind(db.nearby_monsters()))
```

This keeps table message registration eager while table callback binding stays lazy and connection-scoped.

## Messages

Depending on the table shape, `bevy_stdb` forwards updates into Bevy messages such as:

- `InsertMessage<T>`
- `DeleteMessage<T>`
- `UpdateMessage<T>`
- `InsertUpdateMessage<T>`

This lets normal Bevy systems react to database changes using message readers. These messages include both the affected row data and the SpacetimeDB event that triggered the change.

```rust
use crate::module_bindings::Reducer;
use bevy_stdb::prelude::*;
use spacetimedb_sdk::Event;

fn on_person_insert(mut messages: ReadInsertMessage<PersonRow>) {
  for msg in messages.read() {
    match &msg.event {
      Event::Reducer(r) => {
        /* r.status, r.timestamp, r.reducer */ 
        if let Reducer::CreatePerson(p) = &r.reducer { /* ... */ }
      },
      _ => ={ /* ... */ }
    }
  }
}
```

## Subscriptions

`StdbSubscriptions` stores desired subscription intent separately from the live connection and serves as a lightweight wrapper to manage them.

That means you can:

- enable subscription management during plugin setup using `with_subscriptions`
- queue subscriptions later from normal Bevy systems, typically in response to `StdbConnectedMessage`
- automatically re-apply queued subscription intent after reconnect

Subscriptions are keyed, so you can refer to them using domain-specific identifiers to do things like resubscribe dynamically or unsubscribe. 

There are also messages that are emitted for the `on_applied` and `on_error` callbacks for each subscription. 

```rust
// Check the client cache once a particular subscription has been applied.
fn on_applied(mut applied_msgs: ReadStdbSubscriptionAppliedMessage<SubKey>, conn: Res<StdbConn>) {
  for message in applied_messages.read() {
    if message.is(&SubKey::MyCharacters) {
      println!("You have {} characters.", conn.db().my_characters().count());
    }
  }
}
```

## Reconnects

Reconnect behavior is opt-in. Use `StdbPlugin::with_reconnect` with `StdbReconnectOptions` to enable retry behavior after disconnects. When a reconnect succeeds:

- the `StdbConnection` resource is replaced
- table messages are re-bound
- subscriptions are re-applied

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

## Delayed Connection

Use `with_delayed_connection` when the initial connection should be requested later at runtime instead of during startup.

```rust
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use crate::module_bindings::{DbConnection, RemoteModule};

#[derive(Resource)]
struct ConnectTimer(Timer);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(ConnectTimer(Timer::from_seconds(10.0, TimerMode::Once)))
        .add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_module_name("my_module")
                .with_uri("http://localhost:3000")
                .with_delayed_connection()
                .with_background_driver(DbConnection::run_threaded),
        )
        .add_systems(Update, connect_after_delay)
        .run();
}

fn connect_after_delay(
    time: Res<Time>,
    mut timer: ResMut<ConnectTimer>,
    mut controller: ResMut<StdbConnectionController>,
) {
    if timer.0.tick(time.delta()).just_finished() {
        controller.connect();
    }
}
```

Use `connect_with_token(...)` instead when you want to supply a token at runtime.

### Connection-dependent resources

`bevy_stdb` resources are only available while a connection is active. Systems that access those resources should either be guarded with a run condition such as `resource_exists::<StdbConnection<_>>()`, or accept them as optional system parameters.

This avoids runtime failures when a system runs before the connection has been established or after it has been lost.


## Compatibility

| bevy_stdb | bevy   | spacetimedb_sdk |
| --------- | ------ | --------------- |
| 0.1 - 0.2 | 0.18   | 2.0             |
| 0.3 - 0.7 | 0.18   | 2.1             |

## Notes

This crate focuses on table-driven client workflows. Reducer and procedure access still exist through the active `StdbConnection`, but the primary Bevy-facing event flow is table/message based.

Special thanks to [`bevy_spacetimedb`](https://docs.rs/bevy_spacetimedb/) for the inspiration!

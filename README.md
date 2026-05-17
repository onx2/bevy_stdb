# bevy_stdb

A [Bevy](https://bevy.org/) integration for [SpacetimeDB](https://spacetimedb.com).

[![crates.io](https://img.shields.io/crates/v/bevy_stdb)](https://crates.io/crates/bevy_stdb)
![Dependabot](https://img.shields.io/badge/dependabot-enabled-brightgreen.svg)
[![docs.rs](https://docs.rs/bevy_stdb/badge.svg)](https://docs.rs/bevy_stdb)
[![CI](https://github.com/onx2/bevy_stdb/actions/workflows/ci.yml/badge.svg)](https://github.com/onx2/bevy_stdb/actions/workflows/ci.yml?query=branch%3Amain)
[![CodeQL](https://github.com/onx2/bevy_stdb/actions/workflows/github-code-scanning/codeql/badge.svg)](https://github.com/onx2/bevy_stdb/actions/workflows/github-code-scanning/codeql)

![Useless AI generated image that kind of looks cool](https://github.com/user-attachments/assets/b6cf0408-0c0d-4997-bf9c-e2e0989ab5f3)
_Please enjoy this useless AI generated image based on the README contents of this repo._



## Overview

`bevy_stdb` adapts SpacetimeDB's connection and callback model into Bevy-style resources, systems, plugins, and messages.

## Features

- **Builder-style setup** via `StdbPlugin`
- **Connection resource** access through `StdbConnection`
- **Command interface** for sending SpacetimeDB commands through `StdbCmds`
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
                .with_database_name("my_module")
                .with_uri("http://localhost:3000")
                .add_table::<PlayerInfo>(|reg, db| reg.bind(db.player_info()))
                .with_subscriptions::<MySubKey>()
                .with_reconnect(StdbReconnectOptions::default())
                .with_background_driver(DbConnection::run_threaded),
        )
        .add_systems(Startup, connect)
        .add_systems(Update, (subscribe_on_connect, on_player_info_insert))
        .run();
}

fn connect(mut cmds: StdbCmds) {
    cmds.connect(StdbConnectOptions::default());
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
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use crate::module_bindings::{DbConnection, RemoteModule};

fn main() {
    let stdb_plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_database_name("my_module")
        .with_uri("http://localhost:3000")
        .with_background_driver(DbConnection::run_threaded);
}
```

### Browser / wasm background driving (async)

On browser targets, use the generated background task helper instead:

```rust
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use crate::module_bindings::{DbConnection, RemoteModule};

fn main() {
    let stdb_plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_database_name("my_module")
        .with_uri("http://localhost:3000")
        .with_background_driver(DbConnection::run_background_task)
}
```

If you target both native and browser, I recommend selecting the background driver with `cfg`:

```rust
fn main() {
    let mut stdb_plugin = StdbPlugin::<DbConnection, RemoteModule>::default()
        .with_database_name("my_module")
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
        .with_database_name("my_module")
        .with_uri("http://localhost:3000")
        .with_frame_driver(DbConnection::frame_tick);
}
```

## Table registration

Use the `StdbPlugin` builder methods to register table bindings during app setup.

Each method eagerly registers the Bevy message channels for the row type you specify and stores a deferred binding callback that runs whenever a connection becomes active.

| Method | Use when |
|---|---|
| `add_table` | Table has a primary key — emits insert, update, and delete messages |
| `add_table_without_pk` | Table has no primary key — emits insert and delete messages only |
| `add_event_table` | Append-only log table — emits insert messages only |
| `add_view` | Server-computed virtual table — emits insert and delete messages |

```rust
.add_table::<PlayerInfo>(|reg, db| reg.bind(db.player_info()))
.add_table_without_pk::<WorldClock>(|reg, db| reg.bind(db.world_clock()))
.add_event_table::<DamageEvent>(|reg, db| reg.bind(db.damage_events()))
.add_view::<NearbyMonster>(|reg, db| reg.bind(db.nearby_monsters()))
```

Table message registration happens eagerly at startup; callback binding is deferred until a connection is active.

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
      _ => { /* ... */ }
    }
  }
}
```

## Requesting a connection

By default, start a connection from a Bevy system with `StdbCommands::connect`. To start the initial connection during plugin setup, add `with_eager_connection()` to `StdbPlugin`.

```rust
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use crate::module_bindings::{DbConnection, RemoteModule};

pub type StdbCmds<'w, 's> = StdbCommands<'w, 's, DbConnection, RemoteModule>;

// main fn...

// Use regular bevy system to request a connection via the `StdbCmds` command interface
fn request_connect(mut stdb_cmds: StdbCmds) {
    stdb_cmds.connect(StdbConnectOptions::default());
}
```

## Reconnects

Reconnect behavior is opt-in. Pass `StdbReconnectOptions` to `StdbPlugin::with_reconnect` to enable it.

The reconnect cycle activates when a disconnect message includes an error, or when a connection attempt fails — including a first-time failure. A clean `disconnect()` call does not trigger a retry. While a connection attempt is in-flight the timer is paused; it re-arms once the attempt resolves. The cycle resets fully on a successful connect so the full attempt budget is available again.

```rust
.with_reconnect(StdbReconnectOptions {
    initial_delay: Duration::from_secs(1), // delay before the first retry
    backoff_factor: 1.5,                   // multiplier applied after each failure
    max_delay: Duration::from_secs(15),    // delay is capped at this value
    max_attempts: 0,                       // 0 = retry indefinitely
})
```

When a reconnect succeeds:

- the `StdbConnection` resource is replaced
- table callbacks are re-bound
- subscriptions are re-applied
- 
## Using commands

Use `StdbCommands<C, M>` to connect or disconnect at runtime, optionally overriding the token, URI, or database name configured on the plugin.

```rust
pub type StdbCmds<'w, 's> = StdbCommands<'w, 's, DbConnection, RemoteModule>;

// Connect with plugin defaults:
fn connect(mut cmds: StdbCmds) {
    cmds.connect(StdbConnectOptions::default());
}

// Connect with a runtime token override:
fn connect_with_token(mut cmds: StdbCmds) {
    cmds.connect(StdbConnectOptions::from_token("json.web.token"));
}
```

See `StdbConnectOptions` for all available overrides (`from_token`, `from_uri`, `from_database_name`, `from_target`).

### Connection-dependent resources

`bevy_stdb` resources are only available while a connection is active. Guard systems with `resource_exists::<StdbConnection<_>>()` or accept the connection as an optional parameter. If you need to detect that a connection has been lost before the resource is cleaned up, `StdbConnection::is_active()` checks whether the underlying send channel is still open:

```rust
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use crate::module_bindings::{DbConnection, RemoteModule};

pub type StdbConn = StdbConnection<DbConnection>;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_database_name("my_module")
                .with_uri("http://localhost:3000")
                .with_background_driver(DbConnection::run_threaded),
        )
        .add_systems(
            Update,
            my_system_active.run_if(|conn: Option<Res<StdbConn>>| conn.is_some_and(|c| c.is_active()))
        )
        .add_systems(Update, my_system_option_res)
        .run();
}

fn my_system_active(conn: Res<StdbConn>) {
    // Only runs when StdbConnection resource exists
}

fn my_system_option_res(conn: Option<Res<StdbConn>>) {
    if let Some(conn) = conn {
        // Safe to access connection
    }
}
```

## Subscriptions

Subscriptions are required to tell Spacetime which table data you want to sync to the client. You can directly subscribe using the SDK's standard `subscription_builder` exposed on the connection; however this crate offers a lightweight wrapper to manage them, `StdbSubscriptions`. It stores your desired subscription intent separately from the live connection so they can be reapplied when connections change.

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

## Type Aliases

It is useful to define some type aliases of your own. I suggest making aliases for the connection, subscription, and commands:

```rust
#[derive(Clone, Eq, Hash, PartialEq, Debug)]
pub enum SubKeys {
    PlayerInfo,
    TimeOfDay,
}

pub type StdbConn = StdbConnection<DbConnection>;
pub type StdbSubs = StdbSubscriptions<SubKeys, RemoteModule>;
pub type StdbCmds<'w, 's> = StdbCommands<'w, 's, DbConnection, RemoteModule>;

fn example_system(conn: Res<StdbConn>, mut subs: ResMut<StdbSubs>) {
    let my_table = conn.db().player_info().id().find(&1);
    subs.subscribe_query(SubKeys::TimeOfDay, |q| q.from.world_clock());
}
```


## Compatibility

| bevy_stdb | bevy   | spacetimedb_sdk |
| --------- | ------ | --------------- |
| 0.1 - 0.2 | 0.18   | 2.0             |
| 0.3 - 0.8 | 0.18   | 2.1             |

## Notes

This crate focuses on table-driven client workflows. Reducer and procedure access still exist through the active `StdbConnection`, but the primary Bevy-facing event flow is table/message based.

Special thanks to [`bevy_spacetimedb`](https://docs.rs/bevy_spacetimedb/) for the inspiration!

# bevy_stdb

A [Bevy](https://bevy.org/) integration for [SpacetimeDB](https://spacetimedb.com).

[![crates.io](https://img.shields.io/crates/v/bevy_stdb)](https://crates.io/crates/bevy_stdb)
![Dependabot](https://img.shields.io/badge/dependabot-enabled-brightgreen.svg)
[![docs.rs](https://docs.rs/bevy_stdb/badge.svg)](https://docs.rs/bevy_stdb)
[![CI](https://github.com/onx2/bevy_stdb/actions/workflows/ci.yml/badge.svg)](https://github.com/onx2/bevy_stdb/actions/workflows/ci.yml?query=branch%3Amain)
[![CodeQL](https://github.com/onx2/bevy_stdb/actions/workflows/github-code-scanning/codeql/badge.svg)](https://github.com/onx2/bevy_stdb/actions/workflows/github-code-scanning/codeql)




## Overview

`bevy_stdb` adapts SpacetimeDB's connection and callback model into Bevy-style resources, systems, plugins, and messages.

## Features

- **Builder-style setup** via `StdbPlugin`
- **Connection resource** access through `StdbConnection`
- **Command interface** for sending SpacetimeDB commands through `StdbCmds`
- **Table event bridging** through Bevy `MessageReader` aliases
- **Callback bridging** for reducer/procedure completions through `StdbChannels` and `add_channel_message`
- **Managed subscription intent** through `StdbSubscriptions`
- **Optional reconnect support** through `StdbReconnectOptions`, including connections the SDK reports as cleanly closed when the server went away

## Example

```rust
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use crate::module_bindings::{DbConnection, PlayerInfo, PlayerInfoTableAccessor, RemoteModule};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum MySubKey {
    PlayerInfo,
}

pub type StdbConn = StdbConnection<DbConnection>;
pub type StdbSubs = StdbSubscriptions<MySubKey, RemoteModule>;
pub type StdbCmds<'w, 's> = StdbCommands<'w, 's, DbConnection, RemoteModule>;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_database_name("my_module")
                .with_uri("http://localhost:3000")
                .add_table::<PlayerInfoTableAccessor>()
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

These modes are mutually exclusive and in most applications you'll want `with_background_driver(...)`.

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

Use `frame_tick` when you want Bevy to drive connection progress from Bevy each frame. `bevy_stdb` runs this driver in `PreUpdate`, in `StdbSet::Drive`, ahead of `StdbSet::Flush` — so a row the driver delivers is readable in that same frame. An error from the driver other than the connection closing is reported through `ReadStdbDriverErrorMessage`:

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

Each method eagerly registers the internal Bevy message channels for the row type and stores a deferred binding that runs whenever a connection becomes active.

The `add_*` methods are semantic convenience APIs. For capability-based registration, use `bind` or the direct `bind_insert`, `bind_delete`, `bind_update`, and `bind_insert_update` methods. Unsupported capabilities fail at compile time; duplicate bindings panic during plugin configuration with the accessor and capability in the error.

| Method | Use when |
|---|---|
| `add_table::<T>` | Table has a primary key — exposes insert, update, delete, and insert-or-update message readers |
| `add_table_without_pk::<T>` | Table has no primary key — exposes insert and delete message readers |
| `add_event_table::<T>` | Append-only log table — exposes insert message readers |
| `add_view::<T>` | Server-computed virtual table — exposes insert and delete message readers |
| `bind::<T>`, `bind_*::<T>` | Explicit control — exposes specific message readers |

```rust
// Semantic convenience APIs.
.add_table::<PlayerInfoTableAccessor>()
.add_table_without_pk::<WorldClockTableAccessor>()
.add_event_table::<DamageEventsTableAccessor>()
.add_view::<NearbyMonstersTableAccessor>()

// Capability-based API.
.bind::<PlayerInfoTableAccessor>([
    TableCapability::insert(),
    TableCapability::delete(),
    TableCapability::update(),
    TableCapability::insert_update(),
])
.bind_insert::<DamageEventsTableAccessor>()
```

## Reading table events

Depending on the table shape, systems consume database changes through MessageReader aliases:

- `ReadInsertMessage<T>`
- `ReadDeleteMessage<T>`
- `ReadUpdateMessage<T>`
- `ReadInsertUpdateMessage<T>`

These aliases are `MessageReader`s backed by internal message channels. The message types themselves are not part of the public API, so application code can observe table events without writing them directly. Values yielded by `.read()` expose the affected row data and the SpacetimeDB event that triggered the change.

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

The reconnect cycle activates when a connection is lost, or when a connection attempt fails — including a first-time failure. A disconnect you asked for, through `StdbConnection::disconnect` or `StdbCommands`, does not trigger a retry. Disconnect through those rather than an SDK context: a close made with `ctx.disconnect()` in a callback is reported as lost and retried. While a connection attempt is in-flight the timer is paused; it re-arms once the attempt resolves. The cycle resets fully on a successful connect so the full attempt budget is available again. `StdbCommands::connect`, `reconnect`, and `disconnect` also end the cycle: a disconnect cancels a retry that was already counting down, and a connect after the cycle gave up gets the full budget again.

"Lost" is decided by `bevy_stdb`, not by the SDK. The SDK reports a server that went away as a disconnect with no error — exactly what it reports for a disconnect you requested — so the absence of an error cannot mean "intentional". Each connection instead remembers whether this client asked it to close, and `StdbDisconnectedMessage::result` says which it was:

```rust
fn on_disconnected(mut messages: ReadStdbDisconnectedMessage) {
    for msg in messages.read() {
        match &msg.result {
            Ok(DisconnectIntent::Requested) => { /* we closed it; nothing retries */ }
            Ok(DisconnectIntent::Lost) => { /* it went away; reconnect will retry */ }
            Err(err) => warn!("connection failed: {err}"),
        }
    }
}
```

The backoff counts wall-clock time (`Time<Real>`), so a paused or sped-up game reconnects on the same schedule. When `max_attempts` is reached the cycle stops and says so once through `ReadStdbReconnectExhaustedMessage`.

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
- table callbacks are re-bound, before the new connection can deliver a row
- subscriptions are re-applied

Each reconnect is a new SpacetimeDB connection with an empty client cache. The re-applied subscriptions deliver every matching row again as an insert, and rows deleted while the client was offline never arrive as deletes. If you mirror a table into entities or resources, clear the mirror when `ReadStdbConnectedMessage` reports a connection, before applying that frame's table changes.

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

See `StdbConnectOptions` for all available overrides (`from_token`, `from_uri`, `from_database_name`, `from_target`). An override is not one-shot: it replaces the stored setting, so later attempts and automatic reconnects keep using it.

### Connection-dependent resources

`StdbConnection` only exists while there is a connection. Guard systems with `resource_exists::<StdbConnection<_>>()` or accept the connection as an optional parameter. If you need to detect that a connection has been lost before the resource is cleaned up, `StdbConnection::is_active()` checks whether the underlying send channel is still open:

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
- queue subscriptions later from normal Bevy systems, typically by reading `ReadStdbConnectedMessage`
- automatically re-apply queued subscription intent after reconnect

Handles belong to the connection that issued them, so `StdbSubscriptions` follows the `StdbConnection` resource: it re-applies every stored query to each new connection and drops the old handles once the connection is gone. It does not wait to be told the old one closed. Calling `subscribe_sql` again with a query that is already queued or live does nothing; one whose subscription ended, because it failed, is queued again, so a failed subscription can be retried as-is.

Subscriptions are keyed, so you can refer to them using domain-specific identifiers to do things like resubscribe dynamically or unsubscribe. 

Subscription callbacks are exposed through `ReadStdbSubscriptionAppliedMessage<K>` and `ReadStdbSubscriptionErrorMessage<K>`.

```rust
// Check the client cache once a particular subscription has been applied.
fn on_applied(mut applied_messages: ReadStdbSubscriptionAppliedMessage<SubKey>, conn: Res<StdbConn>) {
  for message in applied_messages.read() {
    if message.is(&SubKey::MyCharacters) {
      println!("You have {} characters.", conn.db().my_characters().count());
    }
  }
}
```

## Reducer and procedure callbacks

SpacetimeDB's `<reducer>_then(...)` _(and procedure)_ callbacks run outside of the Bevy world, so you can't use `SystemParam`s in them. If you need other clients to hear about the result of a reducer or procedure call, you should use `EventTable`s; however, sometimes there are local-only changes that need a guaranteed response from the bevy world. This is where custom channel messages come in handy: `.add_channel_message::<T>()` lets you define a message type that can be sent from outside the Bevy world and read in a normal system.

1. `#[derive(Message)]` on your payload and register it with `add_channel_message::<T>()`
2. In a system, clone a sender using `channels.sender::<T>()`, then move it into the callback
3. Read it with `MessageReader<T>`, allowing `SystemParam`s to be used in response to that callback

```rust
use bevy_stdb::prelude::*;
use spacetimedb_sdk::__codegen::InternalError;

// The reducer result type isn't part of bevy_stdb, but comes in handy to define in your project
type ReducerResult = Result<Result<(), String>, InternalError>;

// The custom message you want to send from the reducer or procedure callback
#[derive(Message)]
pub struct CharacterCreated {
    pub result: ReducerResult,
}

// During plugin setup, register the message type with .add_channel_message::<CharacterCreated>()
// .add_channel_message::<CharacterCreated>()

// A system that listens for some UI action for example, and sends your message when the SpacetimeDB callback finishes
fn create_character(conn: Res<StdbConn>, channels: Res<StdbChannels>) {
    let tx = channels.sender::<CharacterCreated>();
    let _ = conn.reducers().create_character_then("Bob".into(), move |_ctx, result| {
        let _ = tx.send(CharacterCreated { result });
    });
}

// Now we can use a regular system to handle the message, update UI, navigate to a new state, etc.
fn on_character_created(mut messages: MessageReader<CharacterCreated>, mut commands: Commands) {
    for created in messages.read() {
        match &created.result {
            Ok(Ok(())) => { /* commands.spawn(...), next_state.set_state(...), etc... */ }
            Ok(Err(reason)) => warn!("rejected: {reason}"),
            Err(err) => error!("internal error: {err}"),
        }
    }
}
```

Using this for reducers and procedures is the typical approach, but this bridge is actually completely generic. The `add_channel_message` method accepts any `Message`, so the same pattern lands the result of anything that finishes elsewhere (procedures, `IoTaskPool` tasks, HTTP responses) back in the Bevy world.

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

| bevy_stdb   | bevy | spacetimedb_sdk | MSRV   |
| ----------- | ---- | --------------- | ------ |
|   0.1 - 0.8 | 0.18 | 2.0 - 2.6       | 1.93   |
|        0.12 | 0.19 | 2.0 - 2.6       | 1.95   |
|        0.13 | 0.19 | 2.7+            | 1.95   |
|        0.14 | 0.20 | 2.7+            | 1.97.1 |


## Notes

This crate focuses on table-driven client workflows. Reducer and procedure access still exist through the active `StdbConnection`, but the primary Bevy-facing flow uses message readers for table events. When you need a reducer/procedure completion (or any off-schedule callback) back in the ECS, bridge it with `add_channel_message` / `StdbChannels` as shown in [Reducer and procedure callbacks](#reducer-and-procedure-callbacks).

Special thanks to [`bevy_spacetimedb`](https://docs.rs/bevy_spacetimedb/) for the inspiration!

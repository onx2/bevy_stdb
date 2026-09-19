# Architecture

How `bevy_stdb` gets rows from a SpacetimeDB module into Bevy systems, and why it is shaped the
way it is. Every diagram here describes code in `src/`; the file and function names are real.

## Where the pieces live

SpacetimeDB's SDK is callback-driven and runs on its own thread. Bevy is a schedule that owns its
world and wants to be polled. The whole crate exists to bridge those two facts without either side
reaching into the other.

```mermaid
flowchart TB
    subgraph db["SpacetimeDB"]
        module["module<br/>tables, reducers, views"]
    end

    subgraph sdk["spacetimedb-sdk — its own thread"]
        conn["DbConnection"]
        cb["row callbacks<br/>on_insert / on_delete / on_update"]
    end

    subgraph bridge["bevy_stdb"]
        chan["crossbeam channel<br/>one per row type<br/>channel_bridge.rs"]
        drain["drain_channels<br/>PreUpdate, StdbSet::Flush"]
        msgs["Messages&lt;TableChange&lt;T&gt;&gt;"]
    end

    subgraph app["your Bevy app"]
        readers["ReadInsertMessage&lt;T&gt;<br/>ReadDeleteMessage&lt;T&gt;<br/>ReadUpdateMessage&lt;T&gt;<br/>ReadInsertUpdateMessage&lt;T&gt;<br/>ReadTableChangeMessage&lt;T&gt;"]
        systems["your systems"]
    end

    module -- "websocket" --> conn
    conn --> cb
    cb -- "send" --> chan
    chan -- "drained once per frame" --> drain
    drain -- "write_batch" --> msgs
    msgs -- "borrowed, filtered" --> readers
    readers --> systems
```

The channel is the thread boundary. Nothing above it touches the Bevy world, and nothing below it
touches the SDK.

## One row change, end to end

The SDK calls a row callback once per changed row, on its own thread, at whatever moment the
transaction arrives. Bevy reads it on the next frame.

```mermaid
sequenceDiagram
    participant M as module
    participant S as SDK thread
    participant C as crossbeam channel
    participant F as PreUpdate · Flush
    participant R as reader system

    M->>S: transaction diff
    loop once per changed row
        S->>S: clone row (and event, unless opted out)
        S->>C: send TableChange::Insert / Delete / Update
    end
    Note over C: queued until the next frame

    F->>C: drain_channels — try_iter
    F->>F: write_batch into Messages<TableChange<T>>
    R->>R: read(), filtering to one change kind
    Note over R: rows are borrowed out of the stream, never copied again
```

Two consequences worth knowing:

- A change becomes visible at the **next `StdbSet::Flush`**, not the instant the SDK delivers it.
  Systems that must see it should run after that set.
- Order is preserved **within one row type**, because every change kind rides the same channel.
  Streams for different row types have no ordering relationship, and the SDK may group or coalesce
  rows while applying a transaction diff, so this is not an operation log.

## What a capability actually binds

A capability is not a stream. It decides which SDK callbacks get bound, and a reader is allowed
once the callbacks it needs are. There are three callbacks and nothing else: `insert_update` is
`insert` plus `update`, not a fourth thing. Binding is idempotent, so however many capabilities
need a callback it is bound once.

```mermaid
flowchart LR
    subgraph caps["capabilities you bind"]
        ci["TableCapability::insert"]
        cd["TableCapability::delete"]
        cu["TableCapability::update"]
        ciu["TableCapability::insert_update"]
    end

    subgraph cbs["SDK callbacks — bound once per accessor"]
        oi["on_insert"]
        od["on_delete"]
        ou["on_update"]
    end

    stream["TableChange&lt;Row&gt;<br/>one stream per row type"]

    subgraph rd["readers this allows"]
        ri["ReadInsertMessage"]
        rdel["ReadDeleteMessage"]
        ru["ReadUpdateMessage"]
        riu["ReadInsertUpdateMessage"]
    end

    ci --> oi
    ciu --> oi
    ciu --> ou
    cu --> ou
    cd --> od

    oi --> stream
    od --> stream
    ou --> stream

    stream --> ri
    stream --> rdel
    stream --> ru
    stream --> riu
```

A reader checks for the callbacks it needs the first time it is read: `ReadInsertMessage` needs
`on_insert`, `ReadInsertUpdateMessage` needs `on_insert` and `on_update`, and so on.

`add_table` binds all three callbacks; `add_table_without_pk` and `add_view` bind insert and
delete; `add_event_table` binds insert only. Reading a view whose callback was never bound
panics naming it — because a filtered view of a stream nobody fed would otherwise read as a table
that simply never changes.

The stream is keyed by **row type**, not accessor. A table and a view over the same row share it,
and a change both of them see arrives twice with nothing to tell them apart.

## Registration happens once, binding happens per connection

These are two different phases, and conflating them is the usual source of confusion. Registration
is app-build time and cannot fail. Binding runs every time a connection becomes active — including
after a reconnect, against fresh SDK table handles.

```mermaid
flowchart TB
    subgraph build["app build — StdbPlugin::build"]
        b1["callbacks recorded in BindLedger"]
        b2["register_channel::&lt;TableChange&lt;Row&gt;&gt;<br/>once per row type"]
        b3["insert RowEventPolicy + BoundStreams"]
        b4["store bind callbacks in StdbTableConfig"]
    end

    subgraph run["when a connection becomes active"]
        r1["bind_tables<br/>called by poll_pending_connection"]
        r2["for each stored callback:<br/>table.on_insert / on_delete / on_update"]
        r3["each closure captures its Sender<br/>and whether this row carries events"]
    end

    b1 --> b2 --> b3 --> b4
    b4 -. "replayed on every connect" .-> r1
    r1 --> r2 --> r3
```

Because the ledger deduplicates by accessor and callback, binding both a table and a view over one
row type registers two SDK callbacks but only one channel and one set of readers.

## Ordering inside PreUpdate

`StdbPlugin` chains five sets. Everything the crate does happens in `PreUpdate`, so by `Update` the
world is consistent.

```mermaid
flowchart LR
    drive["StdbSet::Drive<br/>frame driver only:<br/>frame_tick runs SDK callbacks"]
    flush["StdbSet::Flush<br/>drain every channel"]
    state["StdbSet::StateSync<br/>sync_connection_resource"]
    conn["StdbSet::Connection<br/>poll pending, bind tables, reconnect"]
    subs["StdbSet::Subscriptions<br/>apply queued subscriptions"]
    user["your systems<br/>Update, or PreUpdate after Flush"]

    drive --> flush --> state --> conn --> subs --> user
```

`Drive` comes first because a frame-driven connection runs its SDK callbacks inside `frame_tick`:
ticking ahead of `Flush` means what they send is drained and readable in the same frame. A
background-driven connection sends from its own thread whenever it likes, and `Drive` is empty.

Tables are bound inside `poll_pending_connection`, at the moment a resolved connection is about to
become the `StdbConnection` resource and before its background driver starts. Nothing can deliver
a row to a connection whose callbacks are not yet bound.

Put your own table-reading systems in `Update`, or in `PreUpdate` with `.after(StdbSet::Flush)`.

## Connection lifecycle

Connecting is asynchronous: `StdbCommands::connect` queues a command that starts the attempt as an
`IoTaskPool` task, and a `PendingConnection` resource is polled each frame until it resolves. `StdbConnection` existing is
the signal that the connection is live — which is what re-binds the table callbacks.

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Connecting: connect() or eager_connection
    Connecting --> Connected: pending task resolves, StdbConnection inserted
    Connecting --> Idle: StdbConnectErrorMessage
    Connected --> Disconnected: StdbDisconnectedMessage, StdbConnection removed
    Disconnected --> Connecting: reconnect backoff elapses
    Disconnected --> Idle: no reconnect configured

    note right of Connected
        on the way in, bind_tables
        re-binds the table callbacks;
        subscriptions are re-applied after
    end note
```

Subscriptions are stored as intent in `StdbSubscriptions`, separately from the live connection, so
a reconnect re-applies them without the caller re-subscribing. Each `StdbConnection` carries a
generation number, and `StdbSubscriptions` remembers which generation its handles came from: when
the two differ, every intent is queued again. It deliberately does not wait for a disconnect
message. A frame-driven connection that `StdbCommands::reconnect` replaced is never ticked again,
so it never reports closing, and its successor would otherwise come up with no subscriptions.

### Telling a lost connection from a closed one

The SDK reports a server going away as `on_disconnect` with no error, which is also what it
reports when the client disconnects on purpose. So each connection shares an `AtomicBool` with its
own `on_disconnect` callback; `StdbConnection::disconnect` raises it, and the callback turns that
into `DisconnectIntent::Requested` or `DisconnectIntent::Lost`. Reconnect retries everything but
`Requested`, on a timer that counts `Time<Real>` so that pausing the game does not pause the
network.

## Why the readers are views

Earlier the crate projected each change into its own message type, one per bound capability. That
meant a row was copied once per stream that consumed it, and a system per stream ran every frame to
do the copying.

```mermaid
flowchart LR
    subgraph before["before"]
        cb1["on_insert"]
        cb2["on_insert<br/>(bound a second time)"]
        m1["Messages&lt;InsertMessage&gt;"]
        m2["Messages&lt;InsertUpdateMessage&gt;"]
        cb1 --> m1
        cb2 --> m2
    end

    subgraph after["now"]
        cb3["on_insert<br/>(bound once)"]
        s["Messages&lt;TableChange&gt;"]
        v1["ReadInsertMessage<br/>filters"]
        v2["ReadInsertUpdateMessage<br/>filters"]
        cb3 --> s
        s --> v1
        s --> v2
    end
```

One callback, one copy, one buffer. The readers borrow out of it rather than receiving a copy, so
binding more of them costs nothing per change. The trade is that `TableChange<T>` is an enum sized
by its largest variant, so an app that binds exactly one capability stores a little more per change
than a purpose-built message would — see the Performance section of the README for what that costs.

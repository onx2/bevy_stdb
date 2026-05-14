# Example project

This is a simple example of how to configure a project with Bevy + SpacetimeDB using `bevy_stdb`. It demonstrates a few basic concepts such as type aliases, subscribing to a table, connecting to the local spacetime instace, and sending reducer calls.

## Setup

You'll need the following installed to run this example for native and web. It's very likely you already have Rust and SpacetimeDB, but the `bevy_cli` is a great tool for running bevy applications for native and web.

- Install [rust](https://rust-lang.org/tools/install/)
- Install [spacetimeDB](https://spacetimedb.com/install)
- Install [bevy_cli](https://github.com/TheBevyFlock/bevy_cli#installation)


## Running locally

You can simple run `spacetime dev` from this directory: `bevy_stdb/examples/simple`. This will run the publish + generate commands for SpacetimeDB as well as `bevy run` for the client. If you'd like to run this in the web, you can update the config file:

```diff
"dev": {
-  "run": "bevy run"
+  "run": "bevy run web"
},
  ```

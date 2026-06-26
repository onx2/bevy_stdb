use crate::module_bindings::*;
use bevy::prelude::*;
use bevy_stdb::prelude::*;
use spacetimedb_sdk::__codegen::InternalError;

/// Result of a reducer `_then` callback. The crate doesn't define this — spell
/// it yourself (or inline the type) only where you actually need it.
type ReducerResult = Result<Result<(), String>, InternalError>;

#[derive(Clone, Eq, Hash, PartialEq, Debug)]
pub enum SubKey {
    Player,
}

pub type StdbConn = StdbConnection<DbConnection>;
pub type StdbCmds<'w, 's> = StdbCommands<'w, 's, DbConnection, RemoteModule>;
pub type StdbSubs = StdbSubscriptions<SubKey, RemoteModule>;

/// Message for `move_player` reducer completions, bridged into Bevy from the
/// reducer `_then` callback.
#[derive(Message)]
pub struct MovePlayerDone {
    pub result: ReducerResult,
}

pub struct MyStdbPlugin;
impl Plugin for MyStdbPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_arch = "wasm32")]
        let driver = DbConnection::run_background_task;
        #[cfg(not(target_arch = "wasm32"))]
        let driver = DbConnection::run_threaded;

        app.add_plugins(
            StdbPlugin::<DbConnection, RemoteModule>::default()
                .with_uri(String::from("http://localhost:3000"))
                .with_database_name(String::from("bevy-stdb-simple"))
                .with_subscriptions::<SubKey>()
                .add_table::<Player>(|reg, db| reg.bind(db.player()))
                .add_channel_message::<MovePlayerDone>()
                .with_background_driver(driver),
        );
    }
}

use crate::module_bindings::*;
use bevy::prelude::*;
use bevy_stdb::prelude::*;

#[derive(Clone, Eq, Hash, PartialEq, Debug)]
pub enum SubKey {
    Player,
}

pub type StdbConn = StdbConnection<DbConnection>;
pub type StdbCmds<'w, 's> = StdbCommands<'w, 's, DbConnection, RemoteModule>;
pub type StdbSubs = StdbSubscriptions<SubKey, RemoteModule>;

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
<<<<<<< HEAD
                .bind::<PlayerTableAccessor>([
                    TableCapability::insert(),
                    TableCapability::delete(),
                    TableCapability::update(),
                    TableCapability::insert_update(),
                ])
=======
                .add_table::<PlayerTableAccessor>()
>>>>>>> origin/main
                .with_background_driver(driver),
        );
    }
}

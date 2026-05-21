use crate::connection::{PendingConnection, StdbConnection, StdbConnectionConfig};
use bevy_ecs::{
    prelude::{Commands, Res, World},
    system::{Command, SystemParam},
};
use bevy_tasks::IoTaskPool;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::marker::PhantomData;

/// Options for starting a SpacetimeDB connection attempt.
#[derive(Clone, Debug, Default)]
pub struct StdbConnectOptions {
    /// Optional access token for this connection attempt.
    pub token: Option<String>,
    /// Optional URI for this connection attempt.
    pub uri: Option<String>,
    /// Optional database name for this connection attempt.
    pub database_name: Option<String>,
}

impl StdbConnectOptions {
    /// Creates [`StdbConnectOptions`] with an access token.
    pub fn from_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            uri: None,
            database_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI.
    pub fn from_uri(uri: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            database_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a database name.
    pub fn from_database_name(database_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: None,
            database_name: Some(database_name.into()),
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI and database name.
    pub fn from_target(uri: impl Into<String>, database_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            database_name: Some(database_name.into()),
        }
    }
}

/// Sends SpacetimeDB connection commands from Bevy systems.
#[derive(SystemParam)]
pub struct StdbCommands<'w, 's, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    _config: Res<'w, StdbConnectionConfig<C, M>>,
    commands: Commands<'w, 's>,
}

impl<C, M> StdbCommands<'_, '_, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    /// Requests a SpacetimeDB connection attempt using [`StdbConnectOptions`].
    ///
    /// No-op if a [`StdbConnection`] exists or a connection attempt is already in flight.
    pub fn connect(&mut self, options: StdbConnectOptions) {
        self.commands
            .queue(StartConnectCommand::<C, M>::new(options));
    }

    /// Requests a new connection after closing the active or pending connection.
    pub fn reconnect(&mut self, options: StdbConnectOptions) {
        self.commands.queue(ReconnectCommand::<C, M>::new(options));
    }

    /// Requests disconnection from the active SpacetimeDB connection.
    pub fn disconnect(&mut self) {
        self.commands.queue(DisconnectCommand::<C>::new());
    }
}

/// A command that starts a SpacetimeDB connection attempt.
pub(crate) struct StartConnectCommand<C, M> {
    options: StdbConnectOptions,
    _marker: PhantomData<fn() -> (C, M)>,
}

impl<C, M> StartConnectCommand<C, M> {
    /// Creates [`StartConnectCommand`] with [`StdbConnectOptions`].
    pub(crate) fn new(options: StdbConnectOptions) -> Self {
        Self {
            options,
            _marker: PhantomData,
        }
    }
}

impl<C, M> Command for StartConnectCommand<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    fn apply(self, world: &mut World) {
        if world.contains_resource::<StdbConnection<C>>()
            || world.contains_resource::<PendingConnection<C>>()
        {
            return;
        }

        spawn_connection_task::<C, M>(world, self.options);
    }
}

struct ReconnectCommand<C, M> {
    options: StdbConnectOptions,
    _marker: PhantomData<fn() -> (C, M)>,
}

impl<C, M> ReconnectCommand<C, M> {
    fn new(options: StdbConnectOptions) -> Self {
        Self {
            options,
            _marker: PhantomData,
        }
    }
}

impl<C, M> Command for ReconnectCommand<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    fn apply(self, world: &mut World) {
        disconnect_connection::<C>(world);
        spawn_connection_task::<C, M>(world, self.options);
    }
}

struct DisconnectCommand<C> {
    _marker: PhantomData<fn() -> C>,
}

impl<C> DisconnectCommand<C> {
    fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<C> Command for DisconnectCommand<C>
where
    C: DbContext + Send + Sync + 'static,
{
    fn apply(self, world: &mut World) {
        disconnect_connection::<C>(world);
    }
}

fn spawn_connection_task<C, M>(world: &mut World, options: StdbConnectOptions)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    let config = {
        let mut config = world.resource_mut::<StdbConnectionConfig<C, M>>();

        if let Some(uri) = options.uri {
            config.uri = uri;
        }
        if let Some(database_name) = options.database_name {
            config.database_name = database_name;
        }
        if let Some(token) = options.token {
            config.token = Some(token);
        }

        config.clone()
    };

    let task = IoTaskPool::get().spawn(async move { config.build_connection().await });
    world.insert_resource(PendingConnection::<C>(task));
}

fn disconnect_connection<C>(world: &mut World)
where
    C: DbContext + Send + Sync + 'static,
{
    if let Some(conn) = world.get_resource::<StdbConnection<C>>() {
        let _ = conn.disconnect();
    }

    world.remove_resource::<StdbConnection<C>>();
    world.remove_resource::<PendingConnection<C>>();
}

#[cfg(all(test, not(feature = "browser")))]
mod tests {
    use super::*;
    use crate::{
        channel_bridge::ChannelBridgePlugin,
        connection::{PendingConnection, StdbConnectionConfig, StdbConnectionPlugin},
    };
    use bevy_app::{App, Update};
    use bevy_tasks::TaskPool;
    use spacetimedb_sdk::Compression;
    use std::sync::Arc;

    type TestConnection = test_module::Connection;
    type TestModule = test_module::Module;

    mod test_module {
        use spacetimedb_sdk::{
            __codegen::{self as sdk, DbContextImpl},
            ConnectionId, DbContext, Identity,
        };
        use std::marker::PhantomData;

        #[derive(Debug)]
        pub struct Module;

        #[derive(Default)]
        pub struct DbView;

        impl sdk::InModule for DbView {
            type Module = Module;
        }

        #[derive(Default)]
        pub struct Reducers;

        impl sdk::InModule for Reducers {
            type Module = Module;
        }

        #[derive(Default)]
        pub struct Procedures;

        impl sdk::InModule for Procedures {
            type Module = Module;
        }

        #[derive(Clone, Debug)]
        pub struct Reducer;

        impl sdk::InModule for Reducer {
            type Module = Module;
        }

        impl sdk::Reducer for Reducer {
            fn reducer_name(&self) -> &'static str {
                "test"
            }

            fn args_bsatn(&self) -> Result<Vec<u8>, sdk::__sats::bsatn::EncodeError> {
                Ok(Vec::new())
            }
        }

        #[derive(Default)]
        pub struct Connection {
            db: DbView,
            reducers: Reducers,
            procedures: Procedures,
        }

        impl sdk::InModule for Connection {
            type Module = Module;
        }

        impl sdk::DbConnection for Connection {
            fn new(_imp: DbContextImpl<Module>) -> Self {
                Self::default()
            }
        }

        impl DbContext for Connection {
            type DbView = DbView;
            type Reducers = Reducers;
            type Procedures = Procedures;

            fn db(&self) -> &Self::DbView {
                &self.db
            }

            fn reducers(&self) -> &Self::Reducers {
                &self.reducers
            }

            fn procedures(&self) -> &Self::Procedures {
                &self.procedures
            }

            fn is_active(&self) -> bool {
                false
            }

            fn disconnect(&self) -> sdk::Result<()> {
                Ok(())
            }

            type SubscriptionBuilder = ();

            fn subscription_builder(&self) -> Self::SubscriptionBuilder {}

            fn try_identity(&self) -> Option<Identity> {
                None
            }

            fn connection_id(&self) -> ConnectionId {
                panic!("test connection has no connection id")
            }

            fn try_connection_id(&self) -> Option<ConnectionId> {
                None
            }
        }

        pub struct Context<E> {
            db: DbView,
            reducers: Reducers,
            procedures: Procedures,
            event: E,
        }

        impl<E> sdk::InModule for Context<E> {
            type Module = Module;
        }

        impl<E> DbContext for Context<E> {
            type DbView = DbView;
            type Reducers = Reducers;
            type Procedures = Procedures;

            fn db(&self) -> &Self::DbView {
                &self.db
            }

            fn reducers(&self) -> &Self::Reducers {
                &self.reducers
            }

            fn procedures(&self) -> &Self::Procedures {
                &self.procedures
            }

            fn is_active(&self) -> bool {
                false
            }

            fn disconnect(&self) -> sdk::Result<()> {
                Ok(())
            }

            type SubscriptionBuilder = ();

            fn subscription_builder(&self) -> Self::SubscriptionBuilder {}

            fn try_identity(&self) -> Option<Identity> {
                None
            }

            fn connection_id(&self) -> ConnectionId {
                panic!("test connection has no connection id")
            }

            fn try_connection_id(&self) -> Option<ConnectionId> {
                None
            }
        }

        impl<E: Send + 'static> sdk::AbstractEventContext for Context<E> {
            type Event = E;

            fn event(&self) -> &Self::Event {
                &self.event
            }

            fn new(_imp: DbContextImpl<Module>, event: Self::Event) -> Self {
                Self {
                    db: DbView,
                    reducers: Reducers,
                    procedures: Procedures,
                    event,
                }
            }
        }

        impl sdk::EventContext for Context<sdk::Event<Reducer>> {}
        impl sdk::ReducerEventContext for Context<sdk::ReducerEvent<Reducer>> {}
        impl sdk::ProcedureEventContext for Context<()> {}
        impl sdk::SubscriptionEventContext for Context<()> {}
        impl sdk::ErrorContext for Context<Option<sdk::Error>> {}

        #[derive(Default, Debug)]
        pub struct DbUpdate;

        impl sdk::InModule for DbUpdate {
            type Module = Module;
        }

        impl TryFrom<sdk::__ws::v2::TransactionUpdate> for DbUpdate {
            type Error = sdk::Error;

            fn try_from(_value: sdk::__ws::v2::TransactionUpdate) -> Result<Self, Self::Error> {
                Ok(Self)
            }
        }

        impl sdk::DbUpdate for DbUpdate {
            fn apply_to_client_cache(
                &self,
                _cache: &mut sdk::ClientCache<Module>,
            ) -> AppliedDiff<'_> {
                AppliedDiff(PhantomData)
            }

            fn parse_initial_rows(_raw: sdk::__ws::v2::QueryRows) -> sdk::Result<Self> {
                Ok(Self)
            }

            fn parse_unsubscribe_rows(_raw: sdk::__ws::v2::QueryRows) -> sdk::Result<Self> {
                Ok(Self)
            }
        }

        pub struct AppliedDiff<'r>(PhantomData<&'r ()>);

        impl sdk::InModule for AppliedDiff<'_> {
            type Module = Module;
        }

        impl<'r> sdk::AppliedDiff<'r> for AppliedDiff<'r> {
            fn invoke_row_callbacks(
                &self,
                _event: &Context<sdk::Event<Reducer>>,
                _callbacks: &mut sdk::DbCallbacks<Module>,
            ) {
            }
        }

        #[derive(Clone)]
        pub struct SubscriptionHandle;

        impl sdk::InModule for SubscriptionHandle {
            type Module = Module;
        }

        impl sdk::SubscriptionHandle for SubscriptionHandle {
            fn new(_imp: sdk::SubscriptionHandleImpl<Module>) -> Self {
                Self
            }

            fn is_ended(&self) -> bool {
                false
            }

            fn is_active(&self) -> bool {
                false
            }

            fn unsubscribe_then(self, _on_end: sdk::OnEndedCallback<Module>) -> sdk::Result<()> {
                Ok(())
            }

            fn unsubscribe(self) -> sdk::Result<()> {
                Ok(())
            }
        }

        impl sdk::SpacetimeModule for Module {
            type DbConnection = Connection;
            type EventContext = Context<sdk::Event<Reducer>>;
            type ReducerEventContext = Context<sdk::ReducerEvent<Reducer>>;
            type ProcedureEventContext = Context<()>;
            type SubscriptionEventContext = Context<()>;
            type ErrorContext = Context<Option<sdk::Error>>;
            type Reducer = Reducer;
            type DbView = DbView;
            type Reducers = Reducers;
            type Procedures = Procedures;
            type DbUpdate = DbUpdate;
            type AppliedDiff<'r> = AppliedDiff<'r>;
            type SubscriptionHandle = SubscriptionHandle;
            type QueryBuilder = sdk::QueryBuilder;

            fn register_tables(_client_cache: &mut sdk::ClientCache<Self>) {}

            const ALL_TABLE_NAMES: &'static [&'static str] = &[];
        }
    }

    fn app_with_connection_config() -> App {
        let mut app = App::new();
        app.add_plugins(ChannelBridgePlugin);
        app.add_plugins(StdbConnectionPlugin::<TestConnection, TestModule> {
            database_name: "original_database".to_string(),
            uri: "http://original.invalid".to_string(),
            token: Some("original_token".to_string()),
            eager_connection: false,
            driver: None,
            compression: Compression::default(),
        });
        app
    }

    fn never_finishing_connection_task() -> PendingConnection<TestConnection> {
        let task = IoTaskPool::get_or_init(TaskPool::default).spawn(async {
            std::future::pending::<spacetimedb_sdk::Result<Arc<TestConnection>>>().await
        });
        PendingConnection(task)
    }

    #[test]
    fn connect_commands_reject_same_frame_duplicate_request() {
        fn queue_duplicate_connects(
            mut commands: StdbCommands<'_, '_, TestConnection, TestModule>,
        ) {
            commands.connect(StdbConnectOptions {
                token: Some("first_token".to_string()),
                uri: Some("http://first.invalid".to_string()),
                database_name: Some("first_database".to_string()),
            });
            commands.connect(StdbConnectOptions {
                token: Some("second_token".to_string()),
                uri: Some("http://second.invalid".to_string()),
                database_name: Some("second_database".to_string()),
            });
        }

        let mut app = app_with_connection_config();
        app.add_systems(Update, queue_duplicate_connects);
        app.update();

        assert!(
            app.world()
                .contains_resource::<PendingConnection<TestConnection>>()
        );

        let config = app
            .world()
            .resource::<StdbConnectionConfig<TestConnection, TestModule>>();
        assert_eq!(config.uri, "http://first.invalid");
        assert_eq!(config.database_name, "first_database");
        assert_eq!(config.token.as_deref(), Some("first_token"));
    }

    #[test]
    fn connect_command_rejects_existing_pending_connection_without_mutating_config() {
        let mut app = app_with_connection_config();
        app.world_mut()
            .insert_resource(never_finishing_connection_task());

        StartConnectCommand::<TestConnection, TestModule>::new(StdbConnectOptions {
            token: Some("new_token".to_string()),
            uri: Some("http://new.invalid".to_string()),
            database_name: Some("new_database".to_string()),
        })
        .apply(app.world_mut());

        assert!(
            app.world()
                .contains_resource::<PendingConnection<TestConnection>>()
        );

        let config = app
            .world()
            .resource::<StdbConnectionConfig<TestConnection, TestModule>>();
        assert_eq!(config.uri, "http://original.invalid");
        assert_eq!(config.database_name, "original_database");
        assert_eq!(config.token.as_deref(), Some("original_token"));
    }

    #[test]
    fn disconnect_command_removes_pending_connection() {
        let mut world = World::new();
        world.insert_resource(never_finishing_connection_task());

        DisconnectCommand::<TestConnection>::new().apply(&mut world);

        assert!(!world.contains_resource::<PendingConnection<TestConnection>>());
    }
}

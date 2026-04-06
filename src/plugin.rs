use crate::{
    channel_bridge::ChannelBridgePlugin,
    connection::{ConnectionDriver, StdbConnectionPlugin},
    reconnect::{ReconnectPlugin, StdbReconnectOptions},
    set::StdbSet,
    subscription::{SubscriptionsInitializer, SubscriptionsPlugin},
    table::{
        EventTableBinder, TableBindCallback, TableBinder, TableRegistrationCallback,
        TableWithoutPkBinder, ViewBinder, register_event_table, register_table,
        register_table_without_pk, register_view,
    },
};
use bevy_app::{App, Plugin, PreStartup, PreUpdate};
use bevy_ecs::prelude::IntoScheduleConfigs;
use bevy_state::app::StatesPlugin;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, DbContext, SubscriptionHandle,
};
use std::{hash::Hash, sync::Arc};

/// Primary plugin for configuring `bevy_stdb`.
///
/// # Example
///
/// ```ignore
/// app.add_plugins(
///     StdbPlugin::<DbConnection, Module>::default()
///         .with_module_name("my_module")
///         .with_uri("http://localhost:3000")
///         .with_background_driver(DbConnection::run_threaded)
///         .with_reconnect(StdbReconnectOptions::default())
///         .with_subscriptions::<SubKey>()
///         .add_table::<ChatMessageRow>(|reg, db| reg.bind(db.chat_message()))
/// )
/// .add_systems(Update, subscribe_on_connected);
///
/// fn subscribe_on_connected(
///     mut connected: ReadStdbConnectedMessage,
///     mut subs: ResMut<StdbSubscriptions<SubKey, Module>>,
/// ) {
///     if connected.read().next().is_some() {
///         subs.subscribe_sql(SubKey::Chat, "SELECT * FROM chat_message");
///     }
/// }
/// ```
///
/// # Panics
///
/// Panics during [`Plugin::build`] if required connection settings are
/// missing.
pub struct StdbPlugin<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    module_name: Option<String>,
    uri: Option<String>,
    token: Option<String>,
    compression: Option<Compression>,
    driver: Option<ConnectionDriver<C>>,
    reconnect_options: Option<StdbReconnectOptions>,
    delayed_connection: bool,
    subscriptions_initializer: Option<Arc<SubscriptionsInitializer>>,
    table_registrations: Vec<Arc<TableRegistrationCallback>>,
    table_bindings: Vec<Arc<TableBindCallback<C>>>,
}

impl<C: DbConnection<Module = M> + DbContext + Send + Sync, M: SpacetimeModule<DbConnection = C>>
    Default for StdbPlugin<C, M>
{
    fn default() -> Self {
        Self {
            module_name: None,
            uri: None,
            token: None,
            compression: None,
            driver: None,
            reconnect_options: None,
            delayed_connection: false,
            subscriptions_initializer: None,
            table_registrations: Vec::new(),
            table_bindings: Vec::new(),
        }
    }
}

impl<C: DbConnection<Module = M> + DbContext + Send + Sync, M: SpacetimeModule<DbConnection = C>>
    StdbPlugin<C, M>
{
    /// Sets the function used to drive the connection from the Bevy schedule.
    ///
    /// Use this when you want the active connection to be progressed from Bevy's
    /// schedules instead of in a background task. Internally, `bevy_stdb` runs
    /// this driver from [`PreUpdate`](bevy_app::PreUpdate).
    ///
    /// Exactly one connection driver must be configured for the plugin.
    ///
    /// # Example
    ///
    /// ```ignore
    /// StdbPlugin::<DbConnection, RemoteModule>::default()
    ///     .with_module_name("my_module")
    ///     .with_uri("http://localhost:3000")
    ///     .with_frame_driver(DbConnection::frame_tick)
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if a connection driver has already been configured.
    pub fn with_frame_driver(mut self, frame_tick: fn(&C) -> spacetimedb_sdk::Result<()>) -> Self {
        assert!(
            self.driver.is_none(),
            "`with_frame_driver()` may only be called once"
        );
        self.driver = Some(ConnectionDriver::FrameTick(frame_tick));
        self
    }

    /// Sets the function used to drive the connection in the background.
    ///
    /// Use this when the underlying SDK connection should manage its own
    /// progress outside the Bevy frame loop. The return value of
    /// `background_driver` is ignored.
    ///
    /// Exactly one connection driver must be configured for the plugin.
    ///
    /// # Examples
    ///
    /// Native targets typically use `run_threaded`:
    ///
    /// ```ignore
    /// StdbPlugin::<DbConnection, RemoteModule>::default()
    ///     .with_module_name("my_module")
    ///     .with_uri("http://localhost:3000")
    ///     .with_background_driver(DbConnection::run_threaded)
    /// ```
    ///
    /// Browser targets use the generated async helper instead:
    ///
    /// ```ignore
    /// StdbPlugin::<DbConnection, RemoteModule>::default()
    ///     .with_module_name("my_module")
    ///     .with_uri("http://localhost:3000")
    ///     .with_background_driver(DbConnection::run_background_task)
    /// ```
    ///
    /// To support both, select the driver with `cfg`:
    ///
    /// ```ignore
    /// #[cfg(target_arch = "wasm32")]
    /// let driver = DbConnection::run_background_task;
    /// #[cfg(not(target_arch = "wasm32"))]
    /// let driver = DbConnection::run_threaded;
    ///
    /// StdbPlugin::<DbConnection, RemoteModule>::default()
    ///     .with_module_name("my_module")
    ///     .with_uri("http://localhost:3000")
    ///     .with_background_driver(driver)
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if a connection driver has already been configured.
    pub fn with_background_driver<R>(mut self, background_driver: fn(&C) -> R) -> Self
    where
        R: 'static,
    {
        assert!(
            self.driver.is_none(),
            "`with_background_driver()` may only be called once"
        );
        self.driver = Some(ConnectionDriver::Background(Arc::new(move |conn: &C| {
            let _ = background_driver(conn);
        })));
        self
    }

    /// Sets the remote module name.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn with_module_name(mut self, name: impl Into<String>) -> Self {
        assert!(
            self.module_name.is_none(),
            "`with_module_name()` may only be called once"
        );
        self.module_name = Some(name.into());
        self
    }

    /// Sets the SpacetimeDB host URI.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn with_uri(mut self, uri: impl Into<String>) -> Self {
        assert!(self.uri.is_none(), "`with_uri()` may only be called once");
        self.uri = Some(uri.into());
        self
    }

    /// Sets the authentication token used for the initial connection.
    ///
    /// If [`StdbConnectionController::connect_with_token`](crate::prelude::StdbConnectionController::connect_with_token)
    /// is later used at runtime, the most recently provided token becomes the
    /// stored token used for subsequent reconnect attempts.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        assert!(
            self.token.is_none(),
            "`with_token()` may only be called once"
        );
        self.token = Some(token.into());
        self
    }

    /// Sets the connection compression mode.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn with_compression(mut self, compression: Compression) -> Self {
        assert!(
            self.compression.is_none(),
            "`with_compression()` may only be called once"
        );
        self.compression = Some(compression);
        self
    }

    /// Registers a table with a primary key.
    ///
    /// # Example
    ///
    /// ```ignore
    /// .add_table::<PlayerRow>(|reg, db| reg.bind(db.player_info()))
    /// ```
    pub fn add_table<TRow>(
        mut self,
        bind: impl for<'db> Fn(TableBinder<'_, TRow>, &'db C::DbView) + Send + Sync + 'static,
    ) -> Self
    where
        TRow: Send + Sync + Clone + 'static,
    {
        self.table_registrations
            .push(Arc::new(register_table::<TRow>));
        self.table_bindings.push(Arc::new(move |world, db| {
            let reg = TableBinder::<TRow>::new(world);
            bind(reg, db);
        }));
        self
    }

    /// Registers a table without a primary key.
    ///
    /// # Example
    ///
    /// ```ignore
    /// .add_table_without_pk::<NearbyMonsterRow>(|reg, db| {
    ///     reg.bind(db.nearby_monsters())
    /// })
    /// ```
    pub fn add_table_without_pk<TRow>(
        mut self,
        bind: impl for<'db> Fn(TableWithoutPkBinder<'_, TRow>, &'db C::DbView) + Send + Sync + 'static,
    ) -> Self
    where
        TRow: Send + Sync + Clone + 'static,
    {
        self.table_registrations
            .push(Arc::new(register_table_without_pk::<TRow>));
        self.table_bindings.push(Arc::new(move |world, db| {
            let reg = TableWithoutPkBinder::<TRow>::new(world);
            bind(reg, db);
        }));
        self
    }

    /// Registers a view.
    ///
    /// # Example
    ///
    /// ```ignore
    /// .add_view::<CharacterRow>(|reg, db| reg.bind(db.character_selection_screen_view()))
    /// ```
    pub fn add_view<TRow>(
        mut self,
        bind: impl for<'db> Fn(ViewBinder<'_, TRow>, &'db C::DbView) + Send + Sync + 'static,
    ) -> Self
    where
        TRow: Send + Sync + Clone + 'static,
    {
        self.table_registrations
            .push(Arc::new(register_view::<TRow>));
        self.table_bindings.push(Arc::new(move |world, db| {
            let reg = ViewBinder::<TRow>::new(world);
            bind(reg, db);
        }));
        self
    }

    /// Registers an event table.
    ///
    /// # Example
    ///
    /// ```ignore
    /// .add_event_table::<LogEvent>(|reg, db| reg.bind(db.log_events()))
    /// ```
    pub fn add_event_table<TRow>(
        mut self,
        bind: impl for<'db> Fn(EventTableBinder<'_, TRow>, &'db C::DbView) + Send + Sync + 'static,
    ) -> Self
    where
        TRow: Send + Sync + Clone + 'static,
    {
        self.table_registrations
            .push(Arc::new(register_event_table::<TRow>));
        self.table_bindings.push(Arc::new(move |world, db| {
            let reg = EventTableBinder::<TRow>::new(world);
            bind(reg, db);
        }));
        self
    }

    /// Enables the subscription subsystem.
    ///
    /// This installs [`crate::subscription::StdbSubscriptions`] as a Bevy
    /// resource so subscriptions can be queued at runtime from normal Bevy
    /// systems, for example in response to
    /// [`crate::prelude::StdbConnectedMessage`].
    ///
    /// # Example
    ///
    /// ```ignore
    /// .with_subscriptions::<SubKey>()
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn with_subscriptions<K>(mut self) -> Self
    where
        K: Eq + Hash + Clone + Send + Sync + 'static,
        M::SubscriptionHandle: SubscriptionHandle + Send + Sync + 'static,
        C: DbConnection<Module = M>
            + DbContext<SubscriptionBuilder = spacetimedb_sdk::__codegen::SubscriptionBuilder<M>>
            + Send
            + Sync
            + 'static,
    {
        assert!(
            self.subscriptions_initializer.is_none(),
            "`with_subscriptions()` may only be called once"
        );

        self.subscriptions_initializer = Some(Arc::new(|app: &mut App| {
            app.add_plugins(SubscriptionsPlugin::<K, C, M>::default());
        }));

        self
    }

    /// Enables automatic reconnects with the given options.
    ///
    /// When reconnect is enabled, reconnect attempts use the most recently
    /// stored token. That token comes from either [`Self::with_token`] or a
    /// later runtime call to
    /// [`StdbConnectionController::connect_with_token`](crate::prelude::StdbConnectionController::connect_with_token).
    ///
    /// On a successful reconnect, table callbacks are re-bound and queued
    /// subscriptions are re-applied automatically.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::time::Duration;
    ///
    /// // Use defaults (1s initial delay, 1.5x backoff, 15s max, infinite retries):
    /// .with_reconnect(StdbReconnectOptions::default())
    ///
    /// // Or customize:
    /// .with_reconnect(StdbReconnectOptions {
    ///     initial_delay: Duration::from_secs(2),
    ///     max_attempts: Some(5),
    ///     backoff_factor: 2.0,
    ///     max_delay: Duration::from_secs(30),
    /// })
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn with_reconnect(mut self, reconnect_config: StdbReconnectOptions) -> Self {
        assert!(
            self.reconnect_options.is_none(),
            "`with_reconnect()` may only be called once"
        );
        self.reconnect_options = Some(reconnect_config);
        self
    }

    /// Defers the initial connection until explicitly requested at runtime.
    ///
    /// Row-message registration still happens eagerly during plugin build, but
    /// no connection is started. Call
    /// [`StdbConnectionController::connect`](crate::prelude::StdbConnectionController::connect)
    /// or
    /// [`StdbConnectionController::connect_with_token`](crate::prelude::StdbConnectionController::connect_with_token)
    /// to begin connecting.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // During setup:
    /// StdbPlugin::<DbConnection, RemoteModule>::default()
    ///     .with_module_name("my_module")
    ///     .with_uri("http://localhost:3000")
    ///     .with_delayed_connection()
    ///     .with_background_driver(DbConnection::run_threaded)
    ///
    /// // Later, from a system:
    /// fn connect_on_button_press(mut controller: ResMut<StdbConnectionController>) {
    ///     controller.connect();
    /// }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn with_delayed_connection(mut self) -> Self {
        assert!(
            !self.delayed_connection,
            "`with_delayed_connection()` may only be called once"
        );
        self.delayed_connection = true;
        self
    }
}

impl<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
> Plugin for StdbPlugin<C, M>
{
    /// Installs the configured `bevy_stdb` plugins and resources.
    ///
    /// Exactly one connection driver must be configured via
    /// [`StdbPlugin::with_background_driver`] or [`StdbPlugin::with_frame_driver`].
    ///
    /// # Panics
    ///
    /// Panics if any required configuration is missing:
    ///
    /// - module name
    /// - URI
    /// - connection driver
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<StatesPlugin>() {
            app.add_plugins(StatesPlugin);
        }
        app.add_plugins(ChannelBridgePlugin);

        app.configure_sets(PreStartup, StdbSet::Connection);
        app.configure_sets(
            PreUpdate,
            (
                StdbSet::Flush,
                StdbSet::StateSync,
                StdbSet::Connection,
                StdbSet::Subscriptions,
            )
                .chain(),
        );

        if let Some(reconnect_options) = self.reconnect_options.clone() {
            app.add_plugins(ReconnectPlugin::<C, M>::new(reconnect_options));
        }

        if let Some(init) = self.subscriptions_initializer.clone() {
            init(app);
        }

        for register in &self.table_registrations {
            register(app);
        }

        app.add_plugins(StdbConnectionPlugin::<C, M> {
            module_name: self
                .module_name
                .clone()
                .expect("No module name set. Use with_module_name()"),
            uri: self.uri.clone().expect("No uri set. Use with_uri()"),
            token: self.token.clone(),
            driver: self.driver.clone().or_else(|| {
                panic!(
                    "No connection driver set. Use with_background_driver() or with_frame_driver()"
                )
            }),
            compression: self.compression.unwrap_or_default(),
            delayed_connection: self.delayed_connection,
            table_bindings: self.table_bindings.clone(),
        });
    }
}

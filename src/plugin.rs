//! The main plugin for `bevy_stdb`.
//!
//! [`StdbPlugin`] configures SpacetimeDB connections, table bindings,
//! subscriptions, and reconnect behavior for a Bevy app.

use crate::{
    channel_bridge::ChannelBridgePlugin,
    connection::{ConnectionDriver, StdbConnectionPlugin},
    reconnect::{ReconnectPlugin, StdbReconnectOptions},
    subscription::{StdbSubscriptions, SubscriptionsPlugin},
    table::{
        EventTableBinder, TableBindCallback, TableBinder, TableRegistrationCallback,
        TableWithoutPkBinder, ViewBinder, register_event_table, register_table,
        register_table_without_pk, register_view,
    },
};
use bevy_app::{App, Plugin};
use bevy_state::app::StatesPlugin;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, DbContext, SubscriptionHandle,
};
use std::{hash::Hash, sync::Arc};

type SubscriptionsInitializer = dyn Fn(&mut App) + Send + Sync;

/// Configures the `bevy_stdb` integration for a Bevy app.
///
/// This plugin registers row message channels during app build and binds SDK
/// table callbacks after a connection is established.
///
/// By default, the plugin requests an initial connection during startup. Call
/// [`Self::with_delayed_connection`] to defer connection creation until
/// runtime, then use [`crate::prelude::StdbConnectionController`] to request
/// connection later.
///
/// # Panics
///
/// Plugin installation panics during [`Plugin::build`] if required connection
/// settings are missing:
///
/// - no module name was provided with [`Self::with_module_name`]
/// - no URI was provided with [`Self::with_uri`]
/// - no connection driver was provided with [`Self::with_background_driver`] or
///   [`Self::with_frame_driver`]
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
    /// schedules instead of in a background task.
    ///
    /// Exactly one connection driver must be configured for the plugin.
    ///
    /// # Panics
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
    /// progress outside the Bevy frame loop.
    ///
    /// Exactly one connection driver must be configured for the plugin.
    ///
    /// The return value of `background_driver` is ignored.
    ///
    /// # Panics
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
    /// Panics if called more than once.
    pub fn with_uri(mut self, uri: impl Into<String>) -> Self {
        assert!(self.uri.is_none(), "`with_uri()` may only be called once");
        self.uri = Some(uri.into());
        self
    }

    /// Sets the authentication token used for the initial connection.
    ///
    /// If [`crate::connection::StdbConnectionController::connect_with_token`] is
    /// later used at runtime, the most recently provided token becomes the
    /// stored token used for subsequent reconnect attempts.
    ///
    /// # Panics
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
    /// Panics if called more than once.
    pub fn with_compression(mut self, compression: Compression) -> Self {
        assert!(
            self.compression.is_none(),
            "`with_compression()` may only be called once"
        );
        self.compression = Some(compression);
        self
    }

    /// Adds a table with a primary key.
    ///
    /// This registers the Bevy message channels for `TRow` during plugin build
    /// and stores a deferred binding callback that will attach SDK table
    /// listeners after a connection is established.
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

    /// Adds a table without a primary key.
    ///
    /// This registers the Bevy message channels for `TRow` during plugin build
    /// and stores a deferred binding callback that will attach SDK table
    /// listeners after a connection is established.
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

    /// Adds a view.
    ///
    /// This registers the Bevy message channels for `TRow` during plugin build
    /// and stores a deferred binding callback that will attach SDK table
    /// listeners after a connection is established.
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

    /// Adds an event table.
    ///
    /// This registers the Bevy message channels for `TRow` during plugin build
    /// and stores a deferred binding callback that will attach SDK table
    /// listeners after a connection is established.
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

    /// Enables subscriptions and initializes the stored subscription state.
    ///
    /// The initializer runs during plugin build and can populate the
    /// [`crate::subscription::StdbSubscriptions`] resource with any queries that
    /// should be managed by the subscriptions subsystem.
    ///
    /// # Panics
    /// Panics if called more than once.
    pub fn with_subscriptions<K>(
        mut self,
        init: impl Fn(&mut StdbSubscriptions<K, M>) + Send + Sync + 'static,
    ) -> Self
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

        let init = Arc::new(init);
        self.subscriptions_initializer = Some(Arc::new(move |app: &mut App| {
            let init = init.clone();
            app.add_plugins(SubscriptionsPlugin::<K, C, M>::new(move |subs| {
                init(subs);
            }));
        }));

        self
    }

    /// Enables automatic reconnects with the given options.
    ///
    /// When reconnect is enabled, reconnect attempts use the most recently
    /// stored token. That token comes from either [`Self::with_token`] or a
    /// later runtime call to
    /// [`crate::prelude::StdbConnectionController::connect_with_token`].
    ///
    /// # Panics
    /// Panics if called more than once.
    pub fn with_reconnect(mut self, reconnect_config: StdbReconnectOptions) -> Self {
        assert!(
            self.reconnect_options.is_none(),
            "`with_reconnect()` may only be called once"
        );
        self.reconnect_options = Some(reconnect_config);
        self
    }

    /// Defers creation of the initial connection until it is explicitly
    /// requested at runtime.
    ///
    /// When enabled, plugin setup still performs eager row-message registration,
    /// but no initial connection is requested during startup. Call
    /// [`crate::prelude::StdbConnectionController::connect`] or
    /// [`crate::prelude::StdbConnectionController::connect_with_token`] later to begin
    /// connecting.
    ///
    /// # Panics
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
    /// A connection driver must be configured with exactly one of:
    ///
    /// - [`StdbPlugin::with_background_driver`]
    /// - [`StdbPlugin::with_frame_driver`]
    ///
    /// The configured driver determines how the active connection is progressed
    /// after creation.
    ///
    /// This method performs Bevy-side setup only. It does not rely on plugin
    /// `ready()` hooks. Connection creation is handled later by runtime systems,
    /// eagerly during startup or lazily through
    /// [`crate::prelude::StdbConnectionController`], depending on configuration.
    ///
    /// # Panics
    /// Panics if any required connection configuration is missing:
    ///
    /// - no module name was provided
    /// - no URI was provided
    /// - no connection driver was provided
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<StatesPlugin>() {
            app.add_plugins(StatesPlugin);
        }
        app.add_plugins(ChannelBridgePlugin);

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

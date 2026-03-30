//! The main Bevy plugin for SpacetimeDB integration.
//!
//! This module provides the builder-style entry point for configuring `bevy_stdb`.

use crate::{
    channel_bridge::ChannelBridgePlugin,
    connection::{ConnectionDriver, StdbConnectionPlugin},
    reconnect::{ReconnectPlugin, StdbReconnectOptions},
    subscription::{StdbSubscriptions, SubscriptionsPlugin},
    table::{
        TableBindCallback, TableBinder, TableRegistrar, TableRegistrationCallback,
        register_event_table, register_table, register_table_without_pk, register_view,
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

/// Builder-style plugin for configuring the Bevy-SpacetimeDB integration.
pub struct StdbPlugin<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    // Built-in SpacetimeDB fields
    module_name: Option<String>,
    uri: Option<String>,
    token: Option<String>,
    compression: Option<Compression>,

    // Option for how to process events from the websocket
    driver: Option<ConnectionDriver<C>>,

    // Custom options for reconnect and safely storing sub/table information
    reconnect_options: Option<StdbReconnectOptions>,
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
            driver: None,
            compression: None,
            reconnect_options: None,
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
    /// Exactly one connection driver must be configured for the plugin.
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
    /// Exactly one connection driver must be configured for the plugin.
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
    pub fn with_module_name(mut self, name: impl Into<String>) -> Self {
        assert!(
            self.module_name.is_none(),
            "`with_module_name()` may only be called once"
        );
        self.module_name = Some(name.into());

        self
    }

    /// Sets the SpacetimeDB host URI.
    pub fn with_uri(mut self, uri: impl Into<String>) -> Self {
        assert!(self.uri.is_none(), "`with_uri()` may only be called once");
        self.uri = Some(uri.into());

        self
    }

    /// Sets the authentication token.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        assert!(
            self.token.is_none(),
            "`with_token()` may only be called once"
        );
        self.token = Some(token.into());

        self
    }

    /// Sets the connection compression mode.
    pub fn with_compression(mut self, compression: Compression) -> Self {
        assert!(
            self.compression.is_none(),
            "`with_compression()` may only be called once"
        );
        self.compression = Some(compression);

        self
    }

    /// Registers a table with a primary key using a single bind closure.
    ///
    /// Typical usage:
    ///
    /// ```ignore
    /// .with_table::<PlayerRow>(|bind, db| {
    ///     bind.table(&db.player_info());
    /// })
    /// ```
    pub fn with_table<TRow>(
        mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
    ) -> Self
    where
        TRow: Send + Sync + Clone + 'static,
    {
        self.table_registrations
            .push(Arc::new(register_table::<TRow>));
        self.table_bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));
        self
    }

    /// Registers a table without a primary key using a single bind closure.
    pub fn with_table_without_pk<TRow>(
        mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
    ) -> Self
    where
        TRow: Send + Sync + Clone + 'static,
    {
        self.table_registrations
            .push(Arc::new(register_table_without_pk::<TRow>));
        self.table_bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));
        self
    }

    /// Registers a view using a single bind closure.
    pub fn with_view<TRow>(
        mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
    ) -> Self
    where
        TRow: Send + Sync + Clone + 'static,
    {
        self.table_registrations
            .push(Arc::new(register_view::<TRow>));
        self.table_bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));
        self
    }

    /// Registers an event table using a single bind closure.
    pub fn with_event_table<TRow>(
        mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
    ) -> Self
    where
        TRow: Send + Sync + Clone + 'static,
    {
        self.table_registrations
            .push(Arc::new(register_event_table::<TRow>));
        self.table_bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));
        self
    }

    /// Registers a table with a custom registration builder and a single bind closure.
    ///
    /// This lets you customize which Bevy messages are registered for `TRow`
    /// while still keeping a single runtime bind callback.
    pub fn with_table_build<TRow>(
        mut self,
        bind: impl for<'db> Fn(TableBinder<'_, 'db, C>, &'db C::DbView) + Send + Sync + 'static,
        build: impl FnOnce(&mut TableRegistrar<'_, C>),
    ) -> Self
    where
        TRow: Send + Sync + Clone + 'static,
    {
        let mut registrations = Vec::new();
        let mut bindings = Vec::new();
        let mut registrar = TableRegistrar::<C>::new(&mut registrations, &mut bindings);
        build(&mut registrar);

        self.table_registrations.extend(registrations);
        self.table_bindings.push(Arc::new(move |world, db| {
            let binder = TableBinder::<C>::new(world, db);
            bind(binder, db);
        }));

        self
    }

    /// Enables subscriptions and initializes the stored subscription state.
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
    pub fn with_reconnect(mut self, reconnect_config: StdbReconnectOptions) -> Self {
        assert!(
            self.reconnect_options.is_none(),
            "`with_reconnect()` may only be called once"
        );
        self.reconnect_options = Some(reconnect_config);

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
    /// - `with_background_driver()`
    /// - `with_frame_driver()`
    ///
    /// The configured driver determines how the active connection is progressed
    /// after creation.
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
            table_bindings: self.table_bindings.clone(),
        });
    }
}

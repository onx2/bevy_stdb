//! The main Bevy plugin for SpacetimeDB integration.
//!
//! This module provides the builder-style entry point for configuring `bevy_stdb`.

use crate::{
    channel_bridge::ChannelBridgePlugin,
    connection::StdbConnectionPlugin,
    reconnect::{ReconnectPlugin, StdbReconnectOptions},
    subscription::{StdbSubscriptions, SubscriptionsPlugin},
    table::{TableRegistrar, TableRegistrarCallback},
};
use bevy_app::{App, Plugin};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    Compression, DbContext, SubscriptionHandle,
};
use std::{hash::Hash, sync::Arc, thread::JoinHandle};

type SubscriptionsInitializer = dyn Fn(&mut App) + Send + Sync;

/// Builder-style plugin for configuring the Bevy-SpacetimeDB integration.
pub struct StdbPlugin<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    module_name: Option<String>,
    uri: Option<String>,
    token: Option<String>,
    run_fn: Option<fn(&C) -> JoinHandle<()>>,
    compression: Option<Compression>,
    reconnect_options: Option<StdbReconnectOptions>,
    subscriptions_initializer: Option<Arc<SubscriptionsInitializer>>,
    table_registrar: Option<Arc<TableRegistrarCallback<C>>>,
}

impl<C: DbConnection<Module = M> + DbContext + Send + Sync, M: SpacetimeModule<DbConnection = C>>
    Default for StdbPlugin<C, M>
{
    fn default() -> Self {
        Self {
            module_name: None,
            uri: None,
            token: None,
            run_fn: None,
            compression: Some(Compression::default()),
            reconnect_options: None,
            subscriptions_initializer: None,
            table_registrar: None,
        }
    }
}

impl<C: DbConnection<Module = M> + DbContext + Send + Sync, M: SpacetimeModule<DbConnection = C>>
    StdbPlugin<C, M>
{
    /// Sets the function used to drive the connection.
    pub fn with_run_fn(mut self, run_fn: fn(&C) -> JoinHandle<()>) -> Self {
        assert!(
            self.run_fn.is_none(),
            "`with_run_fn()` may only be called once"
        );
        self.run_fn = Some(run_fn);
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

    /// Registers table callbacks through a [`TableRegistrar`].
    ///
    /// Typical usage:
    ///
    /// ```ignore
    /// .with_tables(|reg, db| {
    ///     reg.table(&db.player_info());
    ///     reg.table_without_pk(&db.nearby_monsters());
    ///     reg.event_table(&db.log_events());
    /// })
    /// ```
    pub fn with_tables(
        mut self,
        register: impl for<'a, 'db> Fn(&mut TableRegistrar<'a>, &'db <C as DbContext>::DbView)
        + Send
        + Sync
        + 'static,
    ) -> Self {
        assert!(
            self.table_registrar.is_none(),
            "`with_tables()` may only be called once"
        );
        self.table_registrar = Some(Arc::new(register));
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
    fn build(&self, app: &mut App) {
        app.add_plugins(ChannelBridgePlugin);

        if let Some(reconnect_options) = self.reconnect_options.clone() {
            app.add_plugins(ReconnectPlugin::<C, M>::new(reconnect_options));
        }

        if let Some(init) = self.subscriptions_initializer.clone() {
            init(app);
        }

        app.add_plugins(StdbConnectionPlugin::<C, M> {
            module_name: self
                .module_name
                .clone()
                .expect("No module name set. Use with_module_name()"),
            uri: self.uri.clone().expect("No uri set. Use with_uri()"),
            token: self.token.clone(),
            run_fn: self.run_fn.expect("No run function set. Use with_run_fn()"),
            compression: self.compression.unwrap_or_default(),
            table_registrar: self.table_registrar.clone(),
        });
    }
}

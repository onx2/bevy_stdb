//! Subscription state and lifecycle for SpacetimeDB.
//!
//! This module stores subscription intent and applies it when a connection is active.

use crate::connection::{StdbConnection, StdbConnectionState};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::{
    prelude::Resource,
    schedule::IntoScheduleConfigs,
    system::{Res, ResMut},
};
use bevy_state::{condition::in_state, state::OnEnter};
use spacetimedb_sdk::{
    __codegen::{__query_builder::Query, DbConnection, SpacetimeModule, SubscriptionBuilder},
    DbContext, Result as StdbResult, SubscriptionHandle as StdbSubscriptionHandle,
};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    marker::PhantomData,
};

type SubscriptionInitializer<K, M> = dyn Fn(&mut StdbSubscriptions<K, M>) + Send + Sync;

/// A [`Resource`] that stores SpacetimeDB subscriptions in Bevy.
///
/// Subscription intent is kept separately from active handles so it can be
/// reapplied after reconnects.
#[derive(Resource)]
pub struct StdbSubscriptions<K, M>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    /// Active handles for the current connection.
    handles: HashMap<K, M::SubscriptionHandle>,
    /// Stored SQL for each subscription key.
    sql_by_key: HashMap<K, String>,
    /// Keys queued to be applied on the next active connection.
    queued_keys: HashSet<K>,
}

impl<K, M> Default for StdbSubscriptions<K, M>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    fn default() -> Self {
        Self {
            handles: HashMap::new(),
            sql_by_key: HashMap::new(),
            queued_keys: HashSet::new(),
        }
    }
}

impl<K, M> StdbSubscriptions<K, M>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    /// Stores a typed query for `key` and queues it to be applied.
    pub fn subscribe_query<T, Q>(&mut self, key: K, query: impl Fn(M::QueryBuilder) -> Q)
    where
        Q: Query<T>,
    {
        let res = query(M::QueryBuilder::default());
        let sql = Query::into_sql(res);
        self.subscribe_sql(key, sql);
    }

    /// Stores a SQL query for `key` and queues it to be applied.
    pub fn subscribe_sql(&mut self, key: K, sql: impl Into<String>) {
        let sql = sql.into();
        self.sql_by_key.insert(key.clone(), sql);
        self.queued_keys.insert(key);
    }

    /// Unsubscribes `key` and removes its stored query.
    pub fn unsubscribe(&mut self, key: &K) -> StdbResult<()> {
        self.sql_by_key.remove(key);
        self.queued_keys.remove(key);
        self.drop_active_handle(key)
    }

    /// Unsubscribes all active handles and clears all stored queries.
    ///
    /// Returns the first unsubscribe error, if any.
    pub fn unsubscribe_all(&mut self) -> StdbResult<()> {
        let mut first_err = None;

        for (_, handle) in self.handles.drain() {
            let Err(err) = handle.unsubscribe() else {
                continue;
            };

            if first_err.is_none() {
                first_err = Some(err);
            }
        }

        self.sql_by_key.clear();
        self.queued_keys.clear();

        if let Some(err) = first_err {
            Err(err)
        } else {
            Ok(())
        }
    }

    /// Returns the stored SQL query for `key`, if any.
    pub fn sql_for(&self, key: &K) -> Option<&str> {
        self.sql_by_key.get(key).map(String::as_str)
    }

    /// Returns `true` if `key` has queued subscription work.
    pub fn is_queued(&self, key: &K) -> bool {
        self.queued_keys.contains(key)
    }

    /// Returns `true` if `key` has an active subscription handle.
    pub fn is_active(&self, key: &K) -> bool {
        self.handles
            .get(key)
            .is_some_and(|handle| handle.is_active())
    }

    /// Queues all stored subscriptions to be applied again.
    pub(crate) fn queue_all(&mut self) {
        for key in self.sql_by_key.keys().cloned() {
            self.queued_keys.insert(key);
        }
    }

    /// Applies queued subscriptions to the active connection.
    pub(crate) fn apply_queued<C>(&mut self, conn: &StdbConnection<C>)
    where
        C: DbConnection<Module = M>
            + DbContext<SubscriptionBuilder = SubscriptionBuilder<M>>
            + Send
            + Sync
            + 'static,
        M: SpacetimeModule<DbConnection = C>,
    {
        let keys: Vec<K> = self.queued_keys.drain().collect();

        for key in keys {
            let Some(sql) = self.sql_by_key.get(&key).cloned() else {
                continue;
            };

            let _ = self.drop_active_handle(&key);
            let handle = conn.subscription_builder().subscribe(sql);
            self.handles.insert(key, handle);
        }
    }

    /// Unsubscribes and removes the active handle for `key` without removing its stored query.
    fn drop_active_handle(&mut self, key: &K) -> StdbResult<()> {
        if let Some(handle) = self.handles.remove(key) {
            handle.unsubscribe()?;
        }
        Ok(())
    }
}

/// Internal plugin for subscription lifecycle management.
pub(crate) struct SubscriptionsPlugin<K, C, M>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    C: DbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SubscriptionBuilder<M>>
        + Send
        + Sync
        + 'static,
    M: SpacetimeModule<DbConnection = C>,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    initializer: Box<SubscriptionInitializer<K, M>>,
    _marker: PhantomData<(C, M)>,
}

impl<K, C, M> SubscriptionsPlugin<K, C, M>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    C: DbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SubscriptionBuilder<M>>
        + Send
        + Sync
        + 'static,
    M: SpacetimeModule<DbConnection = C>,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    pub fn new(initializer: impl Fn(&mut StdbSubscriptions<K, M>) + Send + Sync + 'static) -> Self {
        Self {
            initializer: Box::new(initializer),
            _marker: PhantomData,
        }
    }
}

impl<K, C, M> Plugin for SubscriptionsPlugin<K, C, M>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    C: DbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SubscriptionBuilder<M>>
        + Send
        + Sync
        + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    /// Installs the subscription resource and lifecycle systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<StdbSubscriptions<K, M>>();

        let mut subs = app
            .world_mut()
            .get_resource_mut::<StdbSubscriptions<K, M>>()
            .expect("StdbSubscriptions should be initialized during plugin build");
        (self.initializer)(&mut subs);

        app.add_systems(
            OnEnter(StdbConnectionState::Disconnected),
            queue_subscriptions_on_disconnect::<K, M>,
        );

        app.add_systems(
            PreUpdate,
            apply_queued_subscriptions::<K, C, M>.run_if(in_state(StdbConnectionState::Connected)),
        );
    }
}

/// Re-queues stored subscriptions after a disconnect.
fn queue_subscriptions_on_disconnect<K, M>(mut subs: ResMut<StdbSubscriptions<K, M>>)
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    for (_, handle) in subs.handles.drain() {
        let _ = handle.unsubscribe();
    }

    subs.queue_all();
}

/// Applies queued subscriptions to the current connection.
fn apply_queued_subscriptions<K, C, M>(
    conn: Res<StdbConnection<C>>,
    mut subs: ResMut<StdbSubscriptions<K, M>>,
) where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    C: DbConnection<Module = M>
        + DbContext<SubscriptionBuilder = SubscriptionBuilder<M>>
        + Send
        + Sync
        + 'static,
    M: SpacetimeModule<DbConnection = C>,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    subs.apply_queued(&conn);
}

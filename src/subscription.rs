//! Subscription state and lifecycle management for SpacetimeDB.
//!
//! Manages subscription intent and active handles via Bevy systems and resources.
use crate::connection::{StdbConnection, StdbConnectionState};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Res, ResMut, Resource};
use bevy_state::prelude::OnEnter;
use spacetimedb_sdk::{
    __codegen::{__query_builder::Query, DbConnection, SpacetimeModule, SubscriptionBuilder},
    DbContext, Result as StdbResult, SubscriptionHandle as StdbSubscriptionHandle,
};
use std::{collections::HashMap, hash::Hash, marker::PhantomData};

pub(crate) type SubscriptionsInitializer = dyn Fn(&mut App) + Send + Sync;
type SubscriptionStateInitializer<K, M> = dyn Fn(&mut StdbSubscriptions<K, M>) + Send + Sync;

/// Stored subscription intent and active handle for a single key.
struct SubscriptionEntry<H> {
    /// Active handle for the current connection, if any.
    handle: Option<H>,
    /// Stored SQL query.
    sql: String,
    /// Whether this subscription should be applied on the next active connection.
    queued: bool,
}

/// SpacetimeDB subscription [`Resource`].
///
/// Keeps subscription intent separate from active handles so queries can be
/// reapplied after reconnects.
#[derive(Resource)]
pub struct StdbSubscriptions<K, M>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    /// Subscription entries keyed by user-defined subscription key.
    entries: HashMap<K, SubscriptionEntry<M::SubscriptionHandle>>,
}

impl<K, M> Default for StdbSubscriptions<K, M>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
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

        if let Some(entry) = self.entries.get_mut(&key) {
            entry.sql = sql;
            entry.queued = true;
            return;
        }

        self.entries.insert(
            key,
            SubscriptionEntry {
                handle: None,
                sql,
                queued: true,
            },
        );
    }

    /// Unsubscribes `key` and removes its stored query.
    pub fn unsubscribe(&mut self, key: &K) -> StdbResult<()> {
        let Some(mut entry) = self.entries.remove(key) else {
            return Ok(());
        };

        if let Some(handle) = entry.handle.take() {
            handle.unsubscribe()?;
        }

        Ok(())
    }

    /// Unsubscribes all active handles and clears all stored queries.
    ///
    /// Returns the first unsubscribe error, if any.
    pub fn unsubscribe_all(&mut self) -> StdbResult<()> {
        let mut first_err = None;

        for (_, mut entry) in self.entries.drain() {
            if let Some(handle) = entry.handle.take() {
                let Err(err) = handle.unsubscribe() else {
                    continue;
                };

                if first_err.is_none() {
                    first_err = Some(err);
                }
            }
        }

        if let Some(err) = first_err {
            Err(err)
        } else {
            Ok(())
        }
    }

    /// Returns the stored SQL query for `key`, if any.
    pub fn sql_for(&self, key: &K) -> Option<&str> {
        self.entries.get(key).map(|entry| entry.sql.as_str())
    }

    /// Returns `true` if `key` has queued subscription work.
    pub fn is_queued(&self, key: &K) -> bool {
        self.entries.get(key).is_some_and(|entry| entry.queued)
    }

    /// Returns `true` if `key` has an active subscription handle.
    pub fn is_active(&self, key: &K) -> bool {
        self.entries
            .get(key)
            .and_then(|entry| entry.handle.as_ref())
            .is_some_and(|handle| handle.is_active())
    }

    /// Returns `true` if any subscription has queued work.
    fn has_queued(&self) -> bool {
        self.entries.values().any(|entry| entry.queued)
    }

    /// Sends queued subscriptions to the active connection.
    fn apply_queued<C>(&mut self, conn: &StdbConnection<C>)
    where
        C: DbConnection<Module = M>
            + DbContext<SubscriptionBuilder = SubscriptionBuilder<M>>
            + Send
            + Sync
            + 'static,
        M: SpacetimeModule<DbConnection = C>,
    {
        for entry in self.entries.values_mut() {
            if !entry.queued {
                continue;
            }

            let handle = conn.subscription_builder().subscribe(entry.sql.as_str());
            if let Some(old_handle) = entry.handle.replace(handle) {
                let _ = old_handle.unsubscribe();
            }
            entry.queued = false;
        }
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
    initializer: Box<SubscriptionStateInitializer<K, M>>,
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
    pub(crate) fn new(
        initializer: impl Fn(&mut StdbSubscriptions<K, M>) + Send + Sync + 'static,
    ) -> Self {
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
        let mut subs = app.world_mut().resource_mut::<StdbSubscriptions<K, M>>();
        (self.initializer)(&mut subs);

        app.add_systems(
            OnEnter(StdbConnectionState::Disconnected),
            queue_subscriptions_on_disconnect::<K, M>,
        );

        app.add_systems(
            PreUpdate,
            apply_queued_subscriptions::<K, C, M>.run_if(should_apply_subscriptions::<K, M>),
        );
    }
}

fn should_apply_subscriptions<K, M>(
    subs: Res<StdbSubscriptions<K, M>>,
    state: Res<bevy_state::state::State<StdbConnectionState>>,
) -> bool
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    subs.has_queued() && *state.get() == StdbConnectionState::Connected
}

/// Unsubscribes active handles and re-queues them for the next connection.
fn queue_subscriptions_on_disconnect<K, M>(mut subs: ResMut<StdbSubscriptions<K, M>>)
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    for entry in subs.entries.values_mut() {
        if let Some(handle) = entry.handle.take() {
            let _ = handle.unsubscribe();
            entry.queued = true;
        }
    }
}

/// Sends queued subscriptions to the current connection.
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

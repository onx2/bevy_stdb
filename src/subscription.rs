//! Subscription state and lifecycle management for SpacetimeDB.
//!
//! Manages subscription intent and active handles via Bevy systems and resources.
use crate::{
    channel_bridge::{channel_sender, register_channel},
    connection::{StdbConnection, StdbConnectionState},
    message::{StdbSubscriptionAppliedMessage, StdbSubscriptionErrorMessage},
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Res, ResMut, Resource};
use bevy_state::prelude::{OnEnter, State};
use crossbeam_channel::Sender;
use spacetimedb_sdk::{
    __codegen::{__query_builder::Query, DbConnection, SpacetimeModule, SubscriptionBuilder},
    DbContext, Result as StdbResult, SubscriptionHandle as StdbSubscriptionHandle,
};
use std::{collections::HashMap, hash::Hash, marker::PhantomData};

pub(crate) type SubscriptionsInitializer = dyn Fn(&mut App) + Send + Sync;

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
/// Keeps subscription intent separate from active handles so queued queries can
/// be reapplied after reconnects.
#[derive(Resource)]
pub struct StdbSubscriptions<K, M>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    /// Subscription entries keyed by user-defined subscription key.
    entries: HashMap<K, SubscriptionEntry<M::SubscriptionHandle>>,
    /// Sender for subscription applied lifecycle messages.
    applied_sender: Sender<StdbSubscriptionAppliedMessage<K>>,
    /// Sender for subscription error lifecycle messages.
    error_sender: Sender<StdbSubscriptionErrorMessage<K>>,
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
        for (key, entry) in self.entries.iter_mut().filter(|(_, entry)| entry.queued) {
            let applied_key = key.clone();
            let applied_sender = self.applied_sender.clone();
            let error_key = key.clone();
            let error_sender = self.error_sender.clone();

            let handle = conn
                .subscription_builder()
                .on_applied(move |_ctx| {
                    let _ =
                        applied_sender.send(StdbSubscriptionAppliedMessage { key: applied_key });
                })
                .on_error(move |_ctx, err| {
                    let _ = error_sender.send(StdbSubscriptionErrorMessage {
                        key: error_key,
                        err,
                    });
                })
                .subscribe(entry.sql.as_str());

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
    _marker: PhantomData<(K, C, M)>,
}

impl<K, C, M> Default for SubscriptionsPlugin<K, C, M>
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
    fn default() -> Self {
        Self {
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
        register_channel::<StdbSubscriptionAppliedMessage<K>>(app);
        register_channel::<StdbSubscriptionErrorMessage<K>>(app);

        let world = app.world();
        app.insert_resource(StdbSubscriptions::<K, M> {
            entries: HashMap::default(),
            applied_sender: channel_sender::<StdbSubscriptionAppliedMessage<K>>(world),
            error_sender: channel_sender::<StdbSubscriptionErrorMessage<K>>(world),
        });

        app.add_systems(
            OnEnter(StdbConnectionState::Disconnected),
            queue_subscriptions_on_disconnect::<K, M>,
        );

        app.add_systems(
            PreUpdate,
            apply_queued_subscriptions::<K, C, M>
                .in_set(StdbSet::Subscriptions)
                .run_if(should_apply_subscriptions::<K, M>),
        );
    }
}

fn should_apply_subscriptions<K, M>(
    subs: Res<StdbSubscriptions<K, M>>,
    state: Res<State<StdbConnectionState>>,
) -> bool
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    subs.has_queued() && *state.get() == StdbConnectionState::Connected
}

/// Re-queues all active subscriptions for re-application after a disconnect.
fn queue_subscriptions_on_disconnect<K, M>(mut subs: ResMut<StdbSubscriptions<K, M>>)
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    M: SpacetimeModule,
    M::SubscriptionHandle: StdbSubscriptionHandle + Send + Sync + 'static,
{
    for entry in subs.entries.values_mut() {
        if entry.handle.take().is_some() {
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

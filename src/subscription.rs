//! Subscription state and lifecycle management for SpacetimeDB.
//!
//! Manages subscription intent and active handles via Bevy systems and resources.
use crate::{
    channel_bridge::{channel_sender, register_channel},
    connection::StdbConnection,
    message::{StdbSubscriptionAppliedMessage, StdbSubscriptionErrorMessage},
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Res, ResMut, Resource};
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

/// The part of an SDK subscription handle the intent bookkeeping depends on, so that bookkeeping
/// can be tested without a connection to mint real handles.
trait HandleState {
    /// Whether the subscription is over, by `unsubscribe` or by an error.
    fn is_ended(&self) -> bool;
}

impl<H> HandleState for H
where
    H: StdbSubscriptionHandle,
    H::Module: SpacetimeModule<SubscriptionHandle = H>,
{
    fn is_ended(&self) -> bool {
        StdbSubscriptionHandle::is_ended(self)
    }
}

/// Subscription intent per key, and the connection the live handles belong to.
///
/// A handle is only meaningful on the connection that issued it. Rather than wait to be told a
/// connection ended -- a replaced frame-driven connection is never advanced again and never says
/// so -- this compares the connection it last applied to with the one that exists now.
struct Intents<K, H> {
    entries: HashMap<K, SubscriptionEntry<H>>,
    /// Generation of the connection the handles belong to, or `None` while there was none.
    connection: Option<u64>,
}

impl<K: Eq + Hash, H: HandleState> Intents<K, H> {
    fn new() -> Self {
        Self {
            entries: HashMap::default(),
            connection: None,
        }
    }

    /// Stores `sql` for `key` and queues it, unless that exact query is already queued or live.
    fn subscribe(&mut self, key: K, sql: String) {
        if let Some(entry) = self.entries.get_mut(&key) {
            // An ended handle is not a subscription: a query that failed must be retryable.
            let live = entry.handle.as_ref().is_some_and(|h| !h.is_ended());
            if entry.sql == sql && (entry.queued || live) {
                return;
            }

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

    /// Returns whether [`Self::adopt`] or applying queued entries would do anything.
    fn is_stale(&self, connection: Option<u64>) -> bool {
        self.connection != connection
            || (connection.is_some() && self.entries.values().any(|entry| entry.queued))
    }

    /// Moves to `connection`, re-queueing every intent if it is not the one the handles are from.
    fn adopt(&mut self, connection: Option<u64>) {
        if self.connection == connection {
            return;
        }

        self.connection = connection;
        for entry in self.entries.values_mut() {
            entry.handle = None;
            entry.queued = true;
        }
    }
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
    /// Subscription intent keyed by user-defined subscription key.
    intents: Intents<K, M::SubscriptionHandle>,
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
    ///
    /// Does nothing when `key` already holds this exact query and it is queued or live. A query
    /// whose subscription ended -- it failed, say -- is queued again.
    pub fn subscribe_sql(&mut self, key: K, sql: impl Into<String>) {
        self.intents.subscribe(key, sql.into());
    }

    /// Unsubscribes `key` and removes its stored query.
    pub fn unsubscribe(&mut self, key: &K) -> StdbResult<()> {
        let Some(mut entry) = self.intents.entries.remove(key) else {
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

        for (_, mut entry) in self.intents.entries.drain() {
            if let Some(handle) = entry.handle.take()
                && let Err(err) = handle.unsubscribe()
            {
                first_err.get_or_insert(err);
            }
        }

        first_err.map_or(Ok(()), Err)
    }

    /// Returns the stored SQL query for `key`, if any.
    pub fn sql_for(&self, key: &K) -> Option<&str> {
        self.intents
            .entries
            .get(key)
            .map(|entry| entry.sql.as_str())
    }

    /// Returns `true` if `key` has queued subscription work.
    pub fn is_queued(&self, key: &K) -> bool {
        self.intents
            .entries
            .get(key)
            .is_some_and(|entry| entry.queued)
    }

    /// Returns `true` if `key` has an active subscription handle.
    pub fn is_active(&self, key: &K) -> bool {
        self.intents
            .entries
            .get(key)
            .and_then(|entry| entry.handle.as_ref())
            .is_some_and(|handle| handle.is_active())
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
        let queued = self.intents.entries.iter_mut();
        for (key, entry) in queued.filter(|(_, entry)| entry.queued) {
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
            intents: Intents::new(),
            applied_sender: channel_sender::<StdbSubscriptionAppliedMessage<K>>(world),
            error_sender: channel_sender::<StdbSubscriptionErrorMessage<K>>(world),
        });

        app.add_systems(
            PreUpdate,
            (|conn: Option<Res<StdbConnection<C>>>, mut subs: ResMut<StdbSubscriptions<K, M>>| {
                let generation = conn.as_ref().map(|conn| conn.generation());
                // Checked through a shared borrow first so an idle frame does not mark the
                // resource changed.
                if !subs.intents.is_stale(generation) {
                    return;
                }

                subs.intents.adopt(generation);
                if let Some(conn) = conn {
                    subs.apply_queued(&conn);
                }
            })
            .in_set(StdbSet::Subscriptions),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{HandleState, Intents};

    #[derive(Default)]
    struct FakeHandle {
        ended: bool,
    }

    impl HandleState for FakeHandle {
        fn is_ended(&self) -> bool {
            self.ended
        }
    }

    /// Stands in for `apply_queued`, which needs a connection to mint real handles.
    fn apply(intents: &mut Intents<&'static str, FakeHandle>) -> Vec<&'static str> {
        let mut applied = Vec::new();
        for (key, entry) in intents.entries.iter_mut().filter(|(_, e)| e.queued) {
            entry.handle = Some(FakeHandle::default());
            entry.queued = false;
            applied.push(*key);
        }
        applied
    }

    fn subscribed_on(connection: u64) -> Intents<&'static str, FakeHandle> {
        let mut intents = Intents::new();
        intents.subscribe("players", "SELECT * FROM player".into());
        intents.adopt(Some(connection));
        assert_eq!(apply(&mut intents), ["players"]);
        intents
    }

    #[test]
    fn nothing_is_stale_once_applied() {
        let intents = subscribed_on(1);

        assert!(!intents.is_stale(Some(1)));
    }

    #[test]
    fn a_new_connection_gets_every_subscription_again() {
        // Including a connection that replaced another with no disconnect ever reported, which
        // is what a requested reconnect of a frame-driven connection looks like.
        let mut intents = subscribed_on(1);

        assert!(intents.is_stale(Some(2)));
        intents.adopt(Some(2));

        assert_eq!(apply(&mut intents), ["players"]);
    }

    #[test]
    fn losing_the_connection_queues_every_subscription_until_there_is_one() {
        let mut intents = subscribed_on(1);

        intents.adopt(None);

        assert!(intents.entries["players"].queued);
        assert!(intents.entries["players"].handle.is_none());
        // Queued work alone is not stale: there is nothing to apply it to.
        assert!(!intents.is_stale(None));
    }

    #[test]
    fn the_same_connection_is_adopted_once() {
        let mut intents = subscribed_on(1);

        intents.adopt(Some(1));

        assert!(apply(&mut intents).is_empty());
    }

    #[test]
    fn repeating_a_live_query_changes_nothing() {
        let mut intents = subscribed_on(1);

        intents.subscribe("players", "SELECT * FROM player".into());

        assert!(!intents.is_stale(Some(1)));
    }

    #[test]
    fn a_different_query_for_a_key_is_queued() {
        let mut intents = subscribed_on(1);

        intents.subscribe("players", "SELECT * FROM player WHERE online".into());

        assert!(intents.is_stale(Some(1)));
    }

    #[test]
    fn a_query_whose_subscription_ended_can_be_retried() {
        // A failed subscription keeps its handle. Treating that as live made retrying the same
        // query a no-op for ever.
        let mut intents = subscribed_on(1);
        intents.entries.get_mut("players").unwrap().handle = Some(FakeHandle { ended: true });

        intents.subscribe("players", "SELECT * FROM player".into());

        assert_eq!(apply(&mut intents), ["players"]);
    }
}

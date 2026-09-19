//! Table registration and message forwarding for SpacetimeDB.
//!
//! Registers one [`TableChange`](crate::prelude::TableChange) channel per row type and binds the
//! SDK table callbacks that feed it. The readers in [`reader`](crate::prelude) are views over that
//! channel; nothing here writes a per-kind message.
mod bind;
mod capability;
mod policy;

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::{Resource, World};
pub use bind::{OnDelete, OnInsert, OnUpdate, TableSource};
use capability::BindLedger;
pub use capability::{BoundStreams, RowCallback, TableCapability};
use policy::RowEventPolicy;
use spacetimedb_sdk::__codegen::{DbConnection, DbContext, SpacetimeModule};
use std::{any::TypeId, marker::PhantomData};

/// Performs one-time Bevy app registration for a row type.
pub(crate) type TableRegistration = fn(&mut App);

/// Binds one SpacetimeDB table listener on a concrete database view.
pub(crate) type TableBind<C> = for<'db> fn(&World, &'db <C as DbContext>::DbView);

pub(crate) struct TableRegistry<C: DbContext> {
    table_registrations: Vec<TableRegistration>,
    table_bindings: Vec<TableBind<C>>,
    ledger: BindLedger,
    /// Row types whose messages omit the SDK event.
    event_policy: RowEventPolicy,
}

impl<C: DbContext> Default for TableRegistry<C> {
    fn default() -> Self {
        Self {
            table_registrations: Vec::new(),
            table_bindings: Vec::new(),
            ledger: BindLedger::default(),
            event_policy: RowEventPolicy::default(),
        }
    }
}

impl<C: DbContext> TableRegistry<C> {
    pub(crate) fn plugin<M>(&self) -> StdbTablePlugin<C, M>
    where
        C: DbConnection<Module = M> + Send + Sync + 'static,
        M: SpacetimeModule<DbConnection = C>,
    {
        StdbTablePlugin {
            table_bindings: self.table_bindings.clone(),
            table_registrations: self.table_registrations.clone(),
            event_policy: self.event_policy.clone(),
            bound_streams: self.ledger.bound_streams(),
            _module: PhantomData,
        }
    }

    /// Records that messages for `row_type` omit the SDK event.
    ///
    /// Order-independent: the policy is read when a connection binds its callbacks, long after
    /// every registration has run, so this may be called before or after the table it applies to.
    pub(crate) fn omit_event(&mut self, row_type: TypeId) {
        self.event_policy.omit(row_type);
    }
}

/// Runtime configuration for the SpacetimeDB tables that were registered at build time.
#[derive(Resource)]
struct StdbTableConfig<C: DbContext + 'static> {
    /// Bind functions invoked for each connection that goes live.
    table_bindings: Vec<TableBind<C>>,
}

pub(crate) struct StdbTablePlugin<C: DbContext, M> {
    /// Row types to register before binding to their callbacks.
    table_registrations: Vec<TableRegistration>,
    /// Bind functions invoked for each connection that goes live.
    table_bindings: Vec<TableBind<C>>,
    /// Row types whose messages omit the SDK event.
    event_policy: RowEventPolicy,
    /// The row/callback pairs the table readers may read.
    bound_streams: BoundStreams,
    _module: PhantomData<fn() -> M>,
}

impl<C, M> Plugin for StdbTablePlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    fn build(&self, app: &mut App) {
        for register in &self.table_registrations {
            register(app);
        }

        app.insert_resource(self.event_policy.clone());
        app.insert_resource(self.bound_streams.clone());

        app.insert_resource(StdbTableConfig::<C> {
            table_bindings: self.table_bindings.clone(),
        });
    }
}

/// Binds every registered table callback on `db`, the view of a connection about to go live.
///
/// Runs once per connection, reconnects included, because SDK callbacks belong to the connection
/// they were registered on.
pub(crate) fn bind_tables<C, M>(world: &World, db: &C::DbView)
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    let config = world
        .get_resource::<StdbTableConfig<C>>()
        .expect("StdbTableConfig should exist before a connection is activated");

    for bind in &config.table_bindings {
        bind(world, db);
    }
}

//! Table registration and message forwarding for SpacetimeDB.
//!
//! Registers internal Bevy [`Message`](bevy_ecs::prelude::Message) channels
//! and binds SDK table callbacks for row event readers.
mod bind;
mod capability;

use crate::{connection::StdbConnection, set::StdbSet};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::{
    prelude::{Resource, World, resource_added},
    schedule::IntoScheduleConfigs,
};
pub(crate) use bind::{bind_delete, bind_insert, bind_insert_update, bind_update};
pub use capability::TableCapability;
pub(crate) use capability::TableCapabilityKind;
use spacetimedb_sdk::__codegen::{DbConnection, DbContext, SpacetimeModule};
use std::{any::TypeId, marker::PhantomData, sync::Arc};

/// Stored callback that performs one-time Bevy app registration for a table/view.
pub(crate) type TableRegistrationCallback = dyn Fn(&mut App) + Send + Sync;

/// Stored callback that binds SpacetimeDB table listeners for a concrete database view.
pub(crate) type TableBindCallback<C> =
    dyn for<'db> Fn(&World, &'db <C as DbContext>::DbView) + Send + Sync;

pub(crate) struct TableRegistry<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    table_registrations: Vec<Arc<TableRegistrationCallback>>,
    table_bindings: Vec<Arc<TableBindCallback<C>>>,
    registered_capabilities: Vec<(TypeId, TableCapabilityKind)>,
    _module: PhantomData<fn() -> M>,
}

impl<C, M> Default for TableRegistry<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    fn default() -> Self {
        Self {
            table_registrations: Vec::new(),
            table_bindings: Vec::new(),
            registered_capabilities: Vec::new(),
            _module: PhantomData,
        }
    }
}

impl<C, M> TableRegistry<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    pub(crate) fn plugin(&self) -> StdbTablePlugin<C, M> {
        StdbTablePlugin::new(
            self.table_bindings.clone(),
            self.table_registrations.clone(),
        )
    }
}

/// Runtime configuration for the SpacetimeDB tables that were registered at build time.
#[derive(Resource)]
struct StdbTableConfig<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
> {
    /// Stored bind callbacks invoked for each active connection.
    table_bindings: Vec<Arc<TableBindCallback<C>>>,
}

pub(crate) struct StdbTablePlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C>,
{
    /// Tables to register before binding to their callbacks
    table_registrations: Vec<Arc<TableRegistrationCallback>>,
    /// Stored bind callbacks invoked for each active connection.
    table_bindings: Vec<Arc<TableBindCallback<C>>>,
}
impl<C, M> StdbTablePlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C>,
{
    pub fn new(
        table_bindings: Vec<Arc<TableBindCallback<C>>>,
        table_registrations: Vec<Arc<TableRegistrationCallback>>,
    ) -> Self {
        Self {
            table_bindings,
            table_registrations,
        }
    }
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

        app.insert_resource(StdbTableConfig::<C, M> {
            table_bindings: self.table_bindings.clone(),
        });
        app.add_systems(
            PreUpdate,
            on_connected_bind::<C, M>
                .run_if(resource_added::<StdbConnection<C>>)
                .after(StdbSet::Connection)
                .before(StdbSet::Subscriptions),
        );
    }
}

/// Binds deferred table callbacks after a connection becomes active.
fn on_connected_bind<
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
>(
    world: &mut World,
) {
    let config = world
        .get_resource::<StdbTableConfig<C, M>>()
        .expect("StdbTableConfig should exist before Connected bind phase");
    let conn = world
        .get_resource::<StdbConnection<C>>()
        .expect("StdbConnection should exist before Connected bind phase");

    let db = conn.db();
    for bind in &config.table_bindings {
        bind(&*world, db);
    }
}

use std::marker::PhantomData;

use crate::connection::StdbConnectionConfig;
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, ResMut, resource_exists};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
pub struct StdbAuthPlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync,
    M: SpacetimeModule<DbConnection = C>,
{
    _marker: PhantomData<(C, M)>,
}

impl<C, M> StdbAuthPlugin<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}
impl<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
> Plugin for StdbAuthPlugin<C, M>
{
    fn build(&self, app: &mut App) {
        // - auto-refresh of access_token
        app.add_systems(
            PreUpdate,
            refresh_access_token::<C, M>.run_if(resource_exists::<StdbConnectionConfig<C, M>>),
        );
        // - persist refresh token on app close (bevy shutdown)
    }
}

// Refreshes the access token before it expires
fn refresh_access_token<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    mut conn_config: ResMut<StdbConnectionConfig<C, M>>,
) {
    // TODO;
}

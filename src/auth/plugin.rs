use crate::{auth::StdbTokenResponse, connection::StdbConnectionConfig};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Res, ResMut, Resource, resource_exists};
use bevy_time::{Time, Timer};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::marker::PhantomData;

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
            refresh_access_token::<C, M>
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>)
                .run_if(resource_exists::<StdbTokenResponse>),
        );
        // - persist refresh token on app close (bevy shutdown)
    }
}

#[derive(Resource)]
struct RefreshTimer(Timer);
// Refreshes the access token before it expires
fn refresh_access_token<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    time: Res<Time>,
    mut refresh_timer: ResMut<RefreshTimer>,
    mut token_response: ResMut<StdbTokenResponse>,
    mut conn_config: ResMut<StdbConnectionConfig<C, M>>,
) {
    refresh_timer.0.tick(time.delta());
    if !refresh_timer.0.just_finished() {
        return;
    }
}

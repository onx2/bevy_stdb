use crate::{
    auth::{StdbAuthError, StdbTokenResponse},
    connection::{StdbConnection, StdbConnectionConfig},
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Resource, World, not, resource_exists};
use bevy_tasks::{IoTaskPool, Task, block_on, poll_once};
use bevy_time::{Time, Timer, TimerMode};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::{marker::PhantomData, time::Duration};

const TOKEN_ENDPOINT: &str = "https://auth.spacetimedb.com/oidc/token";
const TOKEN_REFRESH_BUFFER_SECS: u64 = 60;

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
        app.add_systems(
            PreUpdate,
            arm_token_refresh::<C, M>
                .run_if(resource_exists::<StdbAuthRefresh>)
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>)
                .run_if(resource_exists::<StdbConnection<C>>),
        );

        app.add_systems(
            PreUpdate,
            tick_token_refresh::<C, M>
                .run_if(not(resource_exists::<PendingTokenRefresh>))
                .run_if(resource_exists::<StdbAuthRefresh>)
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>)
                .run_if(resource_exists::<StdbConnection<C>>),
        );

        app.add_systems(
            PreUpdate,
            poll_pending_token_refresh::<C, M>
                .run_if(resource_exists::<PendingTokenRefresh>)
                .run_if(resource_exists::<StdbAuthRefresh>)
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>)
                .run_if(resource_exists::<StdbConnection<C>>),
        );
    }
}

#[derive(Resource)]
pub struct StdbAuthRefresh {
    pub refresh_token: String,
    pub refresh_timer: Timer,
}

impl StdbAuthRefresh {
    pub(crate) fn new(refresh_token: impl Into<String>, expires_in_secs: u64) -> Self {
        Self {
            refresh_token: refresh_token.into(),
            refresh_timer: refresh_timer(expires_in_secs),
        }
    }

    pub(crate) fn reset_timer(&mut self, expires_in_secs: u64) {
        self.refresh_timer = refresh_timer(expires_in_secs);
    }

    pub(crate) fn from_token_response(token_response: &StdbTokenResponse) -> Option<Self> {
        let refresh_token = token_response.refresh_token.clone()?;
        let expires_in_secs = token_response.expires_in?;
        Some(Self::new(refresh_token, expires_in_secs))
    }
}

#[derive(Resource)]
struct PendingTokenRefresh(Task<Result<StdbTokenResponse, StdbAuthError>>);

fn refresh_timer(expires_in_secs: u64) -> Timer {
    let refresh_after_secs = expires_in_secs
        .saturating_sub(TOKEN_REFRESH_BUFFER_SECS)
        .max(1);
    Timer::new(Duration::from_secs(refresh_after_secs), TimerMode::Once)
}

fn arm_token_refresh<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    let saw_connected = {
        let connected_msgs = world
            .resource_mut::<bevy_ecs::message::Messages<crate::message::StdbConnectedMessage>>();
        let mut cursor = connected_msgs.get_cursor_current();
        cursor.read(&connected_msgs).next().is_some()
    };

    if !saw_connected {
        return;
    }

    let _ = world.remove_resource::<PendingTokenRefresh>();

    let Some(mut auth_refresh) = world.get_resource_mut::<StdbAuthRefresh>() else {
        return;
    };

    auth_refresh.refresh_timer.reset();
}

fn tick_token_refresh<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    let delta = world.resource::<Time>().delta();

    let Some(client_id) = world
        .get_resource::<StdbConnectionConfig<C, M>>()
        .and_then(|config| config.client_id().map(str::to_owned))
    else {
        return;
    };

    let refresh_token = {
        let Some(mut auth_refresh) = world.get_resource_mut::<StdbAuthRefresh>() else {
            return;
        };

        auth_refresh.refresh_timer.tick(delta);

        if !auth_refresh.refresh_timer.just_finished() {
            return;
        }

        auth_refresh.refresh_token.clone()
    };

    world
        .insert_resource(PendingTokenRefresh(IoTaskPool::get().spawn(async move {
            refresh_token_response(client_id, refresh_token).await
        })));
}

fn poll_pending_token_refresh<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    let Some(PendingTokenRefresh(mut task)) = world.remove_resource::<PendingTokenRefresh>() else {
        return;
    };

    let Some(result) = block_on(poll_once(&mut task)) else {
        world.insert_resource(PendingTokenRefresh(task));
        return;
    };

    let Ok(token_response) = result else {
        return;
    };

    let mut conn_config = world.resource_mut::<StdbConnectionConfig<C, M>>();
    conn_config.update_token(token_response.access_token.clone());

    let Some(mut auth_refresh) = world.get_resource_mut::<StdbAuthRefresh>() else {
        return;
    };

    if let Some(refresh_token) = token_response.refresh_token {
        auth_refresh.refresh_token = refresh_token;
    }

    if let Some(expires_in_secs) = token_response.expires_in {
        auth_refresh.reset_timer(expires_in_secs);
    }
}

async fn refresh_token_response(
    client_id: String,
    refresh_token: String,
) -> Result<StdbTokenResponse, StdbAuthError> {
    let client = reqwest::Client::new();
    let response = client
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
            ("client_id", client_id.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<StdbTokenResponse>()
        .await?;

    Ok(response)
}

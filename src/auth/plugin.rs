use crate::{
    auth::{StdbAuthError, StdbAuthSource, StdbTokenResponse},
    connection::{StdbConnection, StdbConnectionConfig},
    message::{
        StdbLoginFailedMessage, StdbLoginRequest, StdbLoginSucceededMessage, StdbLogoutRequest,
    },
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::prelude::{IntoScheduleConfigs, Messages, Resource, World, not, resource_exists};
use bevy_tasks::{IoTaskPool, Task, block_on, poll_once};
use bevy_time::{Time, Timer, TimerMode};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::{marker::PhantomData, time::Duration};

const TOKEN_ENDPOINT: &str = "https://auth.spacetimedb.com/oidc/token";

#[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
const KEYRING_SERVICE: &str = "bevy_stdb";

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
        app.add_message::<StdbLoginRequest>();
        app.add_message::<StdbLogoutRequest>();
        app.add_message::<StdbLoginSucceededMessage>();
        app.add_message::<StdbLoginFailedMessage>();

        app.add_systems(
            PreUpdate,
            handle_logout_request::<C, M>.run_if(resource_exists::<StdbConnectionConfig<C, M>>),
        );

        app.add_systems(
            PreUpdate,
            handle_login_request
                .run_if(not(resource_exists::<PendingLogin>))
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>),
        );

        app.add_systems(
            PreUpdate,
            poll_pending_login::<C, M>
                .run_if(resource_exists::<PendingLogin>)
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>),
        );

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
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>),
        );

        app.add_systems(
            PreUpdate,
            poll_pending_token_refresh::<C, M>
                .run_if(resource_exists::<PendingTokenRefresh>)
                .run_if(resource_exists::<StdbAuthRefresh>)
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>),
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

struct LoginOutcome {
    token_response: StdbTokenResponse,
    client_id: Option<String>,
}

#[derive(Resource)]
struct PendingLogin(Task<Result<LoginOutcome, StdbAuthError>>);

#[derive(Resource)]
struct PendingTokenRefresh(Task<Result<StdbTokenResponse, StdbAuthError>>);

fn refresh_timer(expires_in_secs: u64) -> Timer {
    let refresh_after_secs = expires_in_secs
        .saturating_sub(TOKEN_REFRESH_BUFFER_SECS)
        .max(1);
    Timer::new(Duration::from_secs(refresh_after_secs), TimerMode::Once)
}

fn handle_login_request(world: &mut World) {
    let Some(request) = world
        .resource_mut::<Messages<StdbLoginRequest>>()
        .drain()
        .last()
    else {
        return;
    };

    let auth_source = request.options.auth_source;
    let client_id = auth_source.client_id();

    world.insert_resource(PendingLogin(IoTaskPool::get().spawn(async move {
        let token_response = acquire_login_token_response(&auth_source).await?;
        Ok(LoginOutcome {
            token_response,
            client_id,
        })
    })));
}

fn poll_pending_login<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    let Some(PendingLogin(mut task)) = world.remove_resource::<PendingLogin>() else {
        return;
    };

    let Some(result) = block_on(poll_once(&mut task)) else {
        world.insert_resource(PendingLogin(task));
        return;
    };

    match result {
        Ok(outcome) => {
            let client_id = outcome.client_id.clone();

            {
                let mut config = world.resource_mut::<StdbConnectionConfig<C, M>>();
                config.update_token(outcome.token_response.access_token.clone());
                config.update_client_id(outcome.client_id);
            }

            if let Some(refresh_token) = outcome.token_response.refresh_token.as_deref() {
                if let Some(client_id) = client_id.as_deref() {
                    store_refresh_token(client_id, refresh_token);
                }
            }

            if let Some(auth_refresh) =
                StdbAuthRefresh::from_token_response(&outcome.token_response)
            {
                world.insert_resource(auth_refresh);
            } else {
                world.remove_resource::<StdbAuthRefresh>();
            }

            world
                .resource_mut::<Messages<StdbLoginSucceededMessage>>()
                .write(StdbLoginSucceededMessage);
        }
        Err(error) => {
            world
                .resource_mut::<Messages<StdbLoginFailedMessage>>()
                .write(StdbLoginFailedMessage {
                    message: format!("{error:?}"),
                });
        }
    }
}

fn handle_logout_request<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    let Some(request) = world
        .resource_mut::<Messages<StdbLogoutRequest>>()
        .drain()
        .last()
    else {
        return;
    };

    let client_id = world
        .get_resource::<StdbConnectionConfig<C, M>>()
        .and_then(|config| config.client_id().map(str::to_owned));

    if request.options.clear_stored_refresh_token {
        if let Some(client_id) = client_id.as_deref() {
            clear_stored_refresh_token(client_id);
        }
    }

    if request.options.clear_memory_session {
        if let Some(mut config) = world.get_resource_mut::<StdbConnectionConfig<C, M>>() {
            config.clear_auth();
        }

        world.remove_resource::<StdbAuthRefresh>();
        world.remove_resource::<PendingLogin>();
        world.remove_resource::<PendingTokenRefresh>();
    }
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

    let client_id = {
        let mut conn_config = world.resource_mut::<StdbConnectionConfig<C, M>>();
        conn_config.update_token(token_response.access_token.clone());
        conn_config.client_id().map(str::to_owned)
    };

    let Some(mut auth_refresh) = world.get_resource_mut::<StdbAuthRefresh>() else {
        return;
    };

    if let Some(refresh_token) = token_response.refresh_token {
        if let Some(client_id) = client_id.as_deref() {
            store_refresh_token(client_id, &refresh_token);
        }

        auth_refresh.refresh_token = refresh_token;
    }

    if let Some(expires_in_secs) = token_response.expires_in {
        auth_refresh.reset_timer(expires_in_secs);
    }
}

async fn acquire_login_token_response(
    auth_source: &StdbAuthSource,
) -> Result<StdbTokenResponse, StdbAuthError> {
    #[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
    if let StdbAuthSource::Oidc(options) = auth_source {
        if let Some(refresh_token) = stored_refresh_token(&options.client_id) {
            match refresh_token_response(options.client_id.clone(), refresh_token).await {
                Ok(token_response) => return Ok(token_response),
                Err(_) => clear_stored_refresh_token(&options.client_id),
            }
        }
    }

    auth_source.acquire_token_response().await
}

#[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
fn stored_refresh_token(client_id: &str) -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, client_id)
        .ok()?
        .get_password()
        .ok()
}

#[cfg(not(all(feature = "auth-oidc", not(feature = "browser"))))]
fn store_refresh_token(_client_id: &str, _refresh_token: &str) {}

#[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
fn store_refresh_token(client_id: &str, refresh_token: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, client_id) {
        let _ = entry.set_password(refresh_token);
    }
}

#[cfg(not(all(feature = "auth-oidc", not(feature = "browser"))))]
fn clear_stored_refresh_token(_client_id: &str) {}

#[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
fn clear_stored_refresh_token(client_id: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, client_id) {
        let _ = entry.delete_credential();
    }
}

async fn refresh_token_response(
    client_id: String,
    refresh_token: String,
) -> Result<StdbTokenResponse, StdbAuthError> {
    #[cfg(not(feature = "browser"))]
    {
        let client = reqwest::blocking::Client::new();
        let response = client
            .post(TOKEN_ENDPOINT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
                ("client_id", client_id.as_str()),
            ])
            .send()?
            .error_for_status()?
            .json::<StdbTokenResponse>()?;

        return Ok(response);
    }

    #[cfg(feature = "browser")]
    {
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

        return Ok(response);
    }
}

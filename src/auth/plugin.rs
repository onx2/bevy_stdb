use crate::{
    auth::{AUTH_URI_BASE, StdbAuthError, StdbAuthSource, StdbTokenResponse},
    connection::{StdbConnection, StdbConnectionConfig},
    message::{
        StdbLoginFailedMessage, StdbLoginSucceededMessage, StdbLogoutFailedMessage,
        StdbLogoutSucceededMessage,
    },
    set::StdbSet,
};
use bevy_app::{App, Plugin, PreUpdate};
use bevy_ecs::{
    prelude::{IntoScheduleConfigs, Resource, World, not, resource_exists},
    schedule::common_conditions::resource_added,
};
use bevy_log::{error, info};
use bevy_tasks::{IoTaskPool, Task, block_on, poll_once};
use bevy_time::{Time, Timer, TimerMode};
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::{marker::PhantomData, time::Duration};

#[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
use crate::auth::oidc::keyring::{
    KEYRING_SERVICE, clear_stored_refresh_token, store_refresh_token, stored_refresh_token,
};

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
        app.add_message::<StdbLoginSucceededMessage>();
        app.add_message::<StdbLoginFailedMessage>();
        app.add_message::<StdbLogoutSucceededMessage>();
        app.add_message::<StdbLogoutFailedMessage>();

        app.add_systems(
            PreUpdate,
            poll_pending_auth::<C, M>
                .run_if(resource_exists::<PendingAuth>)
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>)
                .in_set(StdbSet::PostConnection),
        );

        app.add_systems(
            PreUpdate,
            arm_token_refresh::<C, M>
                .run_if(resource_exists::<StdbAuthRefresh>)
                .run_if(resource_added::<StdbConnection<C>>)
                .in_set(StdbSet::PostConnection),
        );

        app.add_systems(
            PreUpdate,
            tick_token_refresh::<C, M>
                .run_if(not(resource_exists::<PendingTokenRefresh>))
                .run_if(resource_exists::<StdbAuthRefresh>)
                .run_if(resource_exists::<StdbConnection<C>>)
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>)
                .in_set(StdbSet::PostConnection),
        );

        app.add_systems(
            PreUpdate,
            poll_pending_token_refresh::<C, M>
                .run_if(resource_exists::<PendingTokenRefresh>)
                .run_if(resource_exists::<StdbAuthRefresh>)
                .run_if(resource_exists::<StdbConnectionConfig<C, M>>)
                .in_set(StdbSet::PostConnection),
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

pub(crate) struct LoginOutcome {
    pub(crate) token_response: StdbTokenResponse,
    pub(crate) client_id: Option<String>,
}

/// Stores the in-flight task for a pending authentication operation.
#[derive(Resource)]
pub(crate) enum PendingAuth {
    Login(Task<Result<LoginOutcome, StdbAuthError>>),
    Logout {
        task: Task<Result<(), StdbAuthError>>,
        clear_refresh_token: bool,
    },
}

#[derive(Resource)]
pub(crate) struct PendingTokenRefresh(Task<Result<StdbTokenResponse, StdbAuthError>>);

fn refresh_timer(expires_in_secs: u64) -> Timer {
    let refresh_after_secs = expires_in_secs
        .saturating_sub(TOKEN_REFRESH_BUFFER_SECS)
        .max(1);
    Timer::new(Duration::from_secs(refresh_after_secs), TimerMode::Once)
}

fn poll_pending_auth<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    let Some(pending) = world.remove_resource::<PendingAuth>() else {
        return;
    };

    match pending {
        PendingAuth::Login(mut task) => {
            let Some(result) = block_on(poll_once(&mut task)) else {
                world.insert_resource(PendingAuth::Login(task));
                return;
            };

            match result {
                Ok(outcome) => {
                    let client_id = outcome.client_id.clone();

                    {
                        let mut config = world.resource_mut::<StdbConnectionConfig<C, M>>();
                        config.update_token(outcome.token_response.access_token.clone());
                        config.update_client_id(outcome.client_id);
                        config.update_id_token(outcome.token_response.id_token.clone());
                    }

                    if let Some(refresh_token) = outcome.token_response.refresh_token.as_deref() {
                        #[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
                        if let Some(client_id) = client_id.as_deref() {
                            info!("storing OIDC refresh token for client_id={client_id}");
                            store_refresh_token(client_id, refresh_token);
                        }
                    } else {
                        info!("login token response did not include a refresh token");
                    }

                    if let Some(auth_refresh) =
                        StdbAuthRefresh::from_token_response(&outcome.token_response)
                    {
                        world.insert_resource(auth_refresh);
                    } else {
                        world.remove_resource::<StdbAuthRefresh>();
                    }

                    world.write_message_default::<StdbLoginSucceededMessage>();
                }
                Err(error) => {
                    world.write_message(StdbLoginFailedMessage {
                        message: format!("{error:?}"),
                    });
                }
            }
        }

        PendingAuth::Logout {
            mut task,
            clear_refresh_token,
        } => {
            let Some(result) = block_on(poll_once(&mut task)) else {
                world.insert_resource(PendingAuth::Logout {
                    task,
                    clear_refresh_token,
                });
                return;
            };

            if clear_refresh_token {
                #[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
                if let Some(config) = world.get_resource::<StdbConnectionConfig<C, M>>() {
                    if let Some(client_id) = config.client_id() {
                        clear_stored_refresh_token(client_id);
                    }
                }
            }

            if let Some(mut config) = world.get_resource_mut::<StdbConnectionConfig<C, M>>() {
                config.clear_auth();
            }

            world.remove_resource::<StdbAuthRefresh>();
            world.remove_resource::<PendingAuth>();
            world.remove_resource::<PendingTokenRefresh>();

            match result {
                Ok(()) => {
                    world.write_message_default::<StdbLogoutSucceededMessage>();
                }
                Err(error) => {
                    error!("OIDC end-session failed: {error:?}");
                    world.write_message(StdbLogoutFailedMessage {
                        message: format!("{error:?}"),
                    });
                }
            }
        }
    }
}

fn arm_token_refresh<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    world.resource_scope::<StdbAuthRefresh, _>(|world, mut auth_refresh| {
        world.remove_resource::<PendingTokenRefresh>();
        auth_refresh.refresh_timer.reset();
    });
}

fn tick_token_refresh<
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
>(
    world: &mut World,
) {
    let delta = world.resource::<Time>().delta();

    let Some(client_id) = world
        .resource::<StdbConnectionConfig<C, M>>()
        .client_id()
        .map(str::to_owned)
    else {
        return;
    };

    let refresh_token = {
        let mut auth_refresh = world.resource_mut::<StdbAuthRefresh>();

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

    let mut auth_refresh = world.resource_mut::<StdbAuthRefresh>();

    if let Some(refresh_token) = token_response.refresh_token {
        #[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
        if let Some(client_id) = client_id.as_deref() {
            store_refresh_token(client_id, &refresh_token);
        }
        auth_refresh.refresh_token = refresh_token;
    }

    if let Some(expires_in_secs) = token_response.expires_in {
        auth_refresh.reset_timer(expires_in_secs);
    }
}

pub(crate) async fn acquire_login_token_response(
    auth_source: &StdbAuthSource,
) -> Result<StdbTokenResponse, StdbAuthError> {
    #[cfg(all(feature = "auth-oidc", not(feature = "browser")))]
    if let StdbAuthSource::Oidc(options) = auth_source {
        info!(
            "checking keyring service `{KEYRING_SERVICE}` for stored OIDC refresh token for client_id={}",
            options.client_id
        );

        if let Some(refresh_token) = stored_refresh_token(&options.client_id) {
            info!(
                "found stored OIDC refresh token for client_id={}; attempting refresh",
                options.client_id
            );

            match refresh_token_response(options.client_id.clone(), refresh_token).await {
                Ok(token_response) => {
                    info!(
                        "stored OIDC refresh token succeeded for client_id={}",
                        options.client_id
                    );
                    return Ok(token_response);
                }
                Err(error) => {
                    error!(
                        "stored OIDC refresh token failed for client_id={}: {:?}",
                        options.client_id, error
                    );
                    clear_stored_refresh_token(&options.client_id);
                }
            }
        } else {
            info!(
                "no stored OIDC refresh token found for client_id={}; starting interactive login",
                options.client_id
            );
        }
    }

    auth_source.acquire_token_response().await
}

async fn refresh_token_response(
    client_id: String,
    refresh_token: String,
) -> Result<StdbTokenResponse, StdbAuthError> {
    #[cfg(not(feature = "browser"))]
    {
        let client = reqwest::blocking::Client::new();
        let response = client
            .post(format!("{AUTH_URI_BASE}/token"))
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
            .post(format!("{AUTH_URI_BASE}/token"))
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

/// Ends the active SpacetimeDB auth session at the end-session endpoint.
///
/// Best-effort — local auth state is always cleared regardless of the outcome.
pub(crate) async fn end_session(
    client_id: &str,
    id_token: Option<&str>,
) -> Result<(), StdbAuthError> {
    #[cfg(not(feature = "browser"))]
    {
        let mut params: Vec<(&str, &str)> = vec![("client_id", client_id)];
        if let Some(id_token) = id_token {
            params.push(("id_token_hint", id_token));
        }
        let client = reqwest::blocking::Client::new();
        client
            .post(format!("{AUTH_URI_BASE}/session/end"))
            .form(&params)
            .send()?
            .error_for_status()?;
        info!("SpacetimeDB auth session ended successfully");
        return Ok(());
    }

    #[cfg(feature = "browser")]
    {
        error!("browser session end is not implemented yet");
        return Err(StdbAuthError::Internal(
            "browser session end is not implemented yet".to_string(),
        ));
    }
}

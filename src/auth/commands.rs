use super::{
    StdbAuthSource,
    plugin::{
        LoginOutcome, PendingLogin, PendingTokenRefresh, StdbAuthRefresh,
        acquire_login_token_response, clear_stored_refresh_token,
    },
};
use crate::connection::StdbConnectionConfig;
use bevy_ecs::{
    prelude::{Command, Commands, Res, World},
    system::SystemParam,
};
use bevy_tasks::IoTaskPool;
use spacetimedb_sdk::{
    __codegen::{DbConnection, SpacetimeModule},
    DbContext,
};
use std::marker::PhantomData;

/// Options for authenticating with SpacetimeDB.
#[derive(Clone, Debug)]
pub struct StdbLoginOptions {
    /// The authentication source used to acquire an access token.
    pub auth_source: StdbAuthSource,
}

impl StdbLoginOptions {
    /// Creates [`StdbLoginOptions`] with the given [`StdbAuthSource`].
    pub fn new(auth_source: StdbAuthSource) -> Self {
        Self { auth_source }
    }
}

/// Options for clearing stored SpacetimeDB authentication.
#[derive(Clone, Debug)]
pub struct StdbLogoutOptions {
    /// Also clears the stored refresh token when `true`.
    pub clear_stored_refresh_token: bool,
}

impl Default for StdbLogoutOptions {
    fn default() -> Self {
        Self {
            clear_stored_refresh_token: true,
        }
    }
}

/// Sends SpacetimeDB auth commands from Bevy systems.
#[derive(SystemParam)]
pub struct StdbAuthCommands<'w, 's, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    config: Res<'w, StdbConnectionConfig<C, M>>,
    pending_login: Option<Res<'w, PendingLogin>>,
    commands: Commands<'w, 's>,
}

/// Deferred command that clears auth state from [`StdbConnectionConfig`].
///
/// Used by [`StdbAuthCommands::logout`] to avoid a `ResMut` conflict when
/// `StdbAuthCommands` and `StdbCommands` are used in the same system.
struct ClearStdbAuthConfig<C, M>(PhantomData<(C, M)>);

impl<C, M> Command for ClearStdbAuthConfig<C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    fn apply(self, world: &mut World) {
        if let Some(mut config) = world.get_resource_mut::<StdbConnectionConfig<C, M>>() {
            config.clear_auth();
        }
    }
}

impl<C, M> StdbAuthCommands<'_, '_, C, M>
where
    C: DbConnection<Module = M> + DbContext + Send + Sync + 'static,
    M: SpacetimeModule<DbConnection = C> + 'static,
{
    /// Spawns a login task immediately using [`StdbLoginOptions`].
    ///
    /// No-op if a login attempt is already pending.
    pub fn login(&mut self, options: StdbLoginOptions) {
        if self.pending_login.is_some() {
            return;
        }
        let auth_source = options.auth_source;
        let client_id = auth_source.client_id();
        let task = IoTaskPool::get().spawn(async move {
            let token_response = acquire_login_token_response(&auth_source).await?;
            Ok(LoginOutcome {
                token_response,
                client_id,
            })
        });
        self.commands.insert_resource(PendingLogin(task));
    }

    /// Clears stored SpacetimeDB authentication using [`StdbLogoutOptions`].
    pub fn logout(&mut self, options: StdbLogoutOptions) {
        if options.clear_stored_refresh_token {
            if let Some(client_id) = self.config.client_id() {
                clear_stored_refresh_token(client_id);
            }
        }
        self.commands
            .queue(ClearStdbAuthConfig::<C, M>(PhantomData));
        self.commands.remove_resource::<StdbAuthRefresh>();
        self.commands.remove_resource::<PendingLogin>();
        self.commands.remove_resource::<PendingTokenRefresh>();
    }
}

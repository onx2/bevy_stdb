use crate::{
    auth::StdbAuthSource,
    message::{StdbConnectRequest, StdbDisconnectRequest, StdbLoginRequest, StdbLogoutRequest},
};
use bevy_ecs::{prelude::MessageWriter, system::SystemParam};

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
    /// Clears the in-memory authentication session when `true`.
    pub clear_memory_session: bool,
    /// Clears the stored refresh token when `true`.
    pub clear_stored_refresh_token: bool,
}

impl Default for StdbLogoutOptions {
    fn default() -> Self {
        Self {
            clear_memory_session: true,
            clear_stored_refresh_token: true,
        }
    }
}

/// Options for starting a SpacetimeDB connection attempt.
#[derive(Clone, Debug, Default)]
pub struct StdbConnectOptions {
    /// Optional access token for this connection attempt.
    pub token: Option<String>,
    /// Optional URI for this connection attempt.
    pub uri: Option<String>,
    /// Optional module name for this connection attempt.
    pub module_name: Option<String>,
}

impl StdbConnectOptions {
    /// Creates [`StdbConnectOptions`] with an access token.
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            uri: None,
            module_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI.
    pub fn with_uri(uri: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            module_name: None,
        }
    }

    /// Creates [`StdbConnectOptions`] with a module name.
    pub fn with_module_name(module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: None,
            module_name: Some(module_name.into()),
        }
    }

    /// Creates [`StdbConnectOptions`] with a URI and module name.
    pub fn with_target(uri: impl Into<String>, module_name: impl Into<String>) -> Self {
        Self {
            token: None,
            uri: Some(uri.into()),
            module_name: Some(module_name.into()),
        }
    }
}

/// Options for disconnecting from SpacetimeDB.
#[derive(Clone, Debug, Default)]
pub struct StdbDisconnectOptions;

/// Sends SpacetimeDB commands from Bevy systems.
#[derive(SystemParam)]
pub struct StdbCommands<'w> {
    login_requests: MessageWriter<'w, StdbLoginRequest>,
    logout_requests: MessageWriter<'w, StdbLogoutRequest>,
    connect_requests: MessageWriter<'w, StdbConnectRequest>,
    disconnect_requests: MessageWriter<'w, StdbDisconnectRequest>,
}

impl StdbCommands<'_> {
    /// Requests authentication using [`StdbLoginOptions`].
    pub fn login(&mut self, options: StdbLoginOptions) {
        self.login_requests.write(StdbLoginRequest {
            auth_source: options.auth_source,
        });
    }

    /// Requests stored authentication to be cleared using [`StdbLogoutOptions`].
    pub fn logout(&mut self, options: StdbLogoutOptions) {
        self.logout_requests.write(StdbLogoutRequest {
            clear_memory_session: options.clear_memory_session,
            clear_stored_refresh_token: options.clear_stored_refresh_token,
        });
    }

    /// Requests a SpacetimeDB connection attempt using [`StdbConnectOptions`].
    pub fn connect(&mut self, options: StdbConnectOptions) {
        self.connect_requests.write(StdbConnectRequest {
            token: options.token,
            uri: options.uri,
            module_name: options.module_name,
        });
    }

    /// Requests disconnection from SpacetimeDB using [`StdbDisconnectOptions`].
    pub fn disconnect(&mut self, _options: StdbDisconnectOptions) {
        self.disconnect_requests.write(StdbDisconnectRequest);
    }
}

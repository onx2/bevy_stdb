use crate::message::{
    AuthFailureMessage, AuthSuccessMessage, RequestLoginMessage, RequestLogoutMessage,
};
use bevy_app::{App, Plugin};
use bevy_ecs::resource::Resource;
use serde::{Deserialize, Serialize};

#[cfg(all(feature = "browser", target_arch = "wasm32"))]
#[path = "web.rs"]
mod auth_imp;

#[cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
#[path = "native.rs"]
mod auth_imp;

#[cfg(all(not(feature = "browser"), target_arch = "wasm32"))]
compile_error!("wasm32 builds require the `browser` feature");

/// Configures OIDC authentication for a SpacetimeDB connection.
#[derive(Clone, Debug)]
pub struct StdbAuthOptions {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The authorization endpoint.
    pub auth_endpoint: String,
    /// The token endpoint.
    pub token_endpoint: String,
    /// The redirect URI used by the client.
    pub redirect_uri: String,
    /// The requested scopes.
    pub scopes: Vec<String>,
}

/// Configures OIDC authentication for a SpacetimeDB connection.
#[derive(Resource, Clone, Debug)]
pub struct StdbAuthConfig {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The authorization endpoint.
    pub auth_endpoint: String,
    /// The token endpoint.
    pub token_endpoint: String,
    /// The redirect URI used by the client.
    pub redirect_uri: String,
    /// The requested scopes.
    pub scopes: Vec<String>,
}

impl From<StdbAuthOptions> for StdbAuthConfig {
    fn from(options: StdbAuthOptions) -> Self {
        Self {
            client_id: options.client_id,
            auth_endpoint: options.auth_endpoint,
            token_endpoint: options.token_endpoint,
            redirect_uri: options.redirect_uri,
            scopes: options.scopes,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenResponse {
    /// The access token used for SpacetimeDB connections.
    pub access_token: String,
    /// The number of seconds before the access token expires.
    pub expires_in: u64,
    /// The optional refresh token.
    pub refresh_token: Option<String>,
    /// The granted scopes.
    pub scope: Option<String>,
    /// The token type.
    pub token_type: String,
    /// The optional ID token.
    pub id_token: Option<String>,
}

/// Installs shared auth resources and systems.
pub(crate) struct StdbAuthPlugin {
    options: StdbAuthOptions,
}

impl StdbAuthPlugin {
    pub(crate) fn new(options: StdbAuthOptions) -> Self {
        Self { options }
    }
}

impl Plugin for StdbAuthPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<RequestLoginMessage>();
        app.add_message::<RequestLogoutMessage>();
        app.add_message::<AuthSuccessMessage>();
        app.add_message::<AuthFailureMessage>();

        app.insert_resource(StdbAuthConfig::from(self.options.clone()));
        // app.init_resource::<CurrentAuthTokens>();
        // app.init_resource::<PendingAuthResume>();
    }
}

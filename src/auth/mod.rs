use bevy_app::{App, Plugin};
use bevy_ecs::prelude::Resource;

#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod auth_imp;

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod auth_imp;

/// Stores the token payload returned by the token endpoint.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct TokenResponse {
    /// The access token used for SpacetimeDB connections.
    pub access_token: String,
    /// The token type - "Bearer" for example
    pub token_type: String,
    /// The ID token for OIDC
    pub id_token: String,
    /// The number of seconds before the access token expires
    pub expires_in: Option<u64>,
    /// The optional refresh token - opaque string for requesting a new access token
    pub refresh_token: Option<String>,
    /// The granted scopes - "openid email profile" for example
    pub scope: Option<String>,
}

/// Configures authentication for a SpacetimeDB connection.
#[derive(Clone, Debug)]
pub enum StdbAuthOptions {
    #[cfg(feature = "auth-oidc")]
    Oidc(OidcOptions),
    #[cfg(feature = "auth-steam")]
    Steam(SteamOptions),
}

#[cfg(feature = "auth-oidc")]
#[derive(Clone, Debug)]
pub struct OidcOptions {
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

#[cfg(feature = "auth-steam")]
#[derive(Clone, Debug)]
pub struct SteamOptions {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The token endpoint.
    pub token_endpoint: String,
    /// The identity of the web service that accepts Steam tickets
    /// For example, "spacetimeauth" when using SpacetimeAuth
    pub ticket_identity: String,
}

/// Stores the configured auth options.
#[derive(Resource, Clone, Debug)]
pub(crate) struct StdbAuthConfig(pub StdbAuthOptions);
impl From<StdbAuthOptions> for StdbAuthConfig {
    fn from(value: StdbAuthOptions) -> Self {
        Self(value)
    }
}

pub(crate) struct StdbAuthPlugin {
    pub options: StdbAuthOptions,
}
impl StdbAuthPlugin {
    pub fn new(options: StdbAuthOptions) -> Self {
        Self { options }
    }
}
impl Plugin for StdbAuthPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(StdbAuthConfig::from(self.options.clone()));
        // TODO
    }
}

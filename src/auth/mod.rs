use bevy_ecs::prelude::Resource;

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

/// Stores the token payload returned by the token endpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct TokenResponse {
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

/// Stores the configured auth options.
#[derive(Resource, Clone, Debug)]
pub(crate) struct StdbAuthConfig {
    /// The configured auth options.
    pub options: StdbAuthOptions,
}

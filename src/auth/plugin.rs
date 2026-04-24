use super::{OidcOptions, SteamOptions};
use bevy_app::{App, Plugin};
use bevy_ecs::prelude::Resource;

/// The configuration for all types of auth
#[derive(Clone, Debug)]
pub struct StdbAuthOptions {
    #[cfg(feature = "auth-oidc")]
    pub oidc: OidcOptions,
    #[cfg(feature = "auth-steam")]
    pub steam: SteamOptions,
}

/// Stores the token payload returned by the token endpoint.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct TokenResponse {
    /// The access token used for SpacetimeDB connections.
    pub access_token: String,
    /// The token type - "Bearer" for example
    pub token_type: String,
    /// The number of seconds before the access token expires
    pub expires_in: Option<u64>,
    /// The optional refresh token - opaque string for requesting a new access token
    pub refresh_token: Option<String>,
    /// The granted scopes - "openid email profile" for example
    pub scope: Option<String>,
    // /// The ID token for OIDC
    // pub id_token: String,
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

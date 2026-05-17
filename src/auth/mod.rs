pub(crate) mod error;
#[cfg(feature = "auth-oidc")]
pub(crate) mod oidc;
pub(crate) mod plugin;
#[cfg(feature = "auth-steam")]
pub(crate) mod steam;

use bevy_ecs::prelude::Resource;
pub(crate) use error::StdbAuthError;
#[cfg(feature = "auth-oidc")]
pub use oidc::{StdbOidcAuthOptions, StdbOidcPrompt};
pub use plugin::StdbAuthPlugin;
#[cfg(feature = "auth-steam")]
pub use steam::StdbSteamAuthOptions;

pub(crate) const AUTH_URI_BASE: &str = "https://auth.spacetimedb.com/oidc";

/// The specific auth target for a given attempt.
#[derive(Clone, Debug)]
pub enum StdbAuthSource {
    #[cfg(feature = "auth-oidc")]
    Oidc(StdbOidcAuthOptions),
    #[cfg(feature = "auth-steam")]
    Steam(StdbSteamAuthOptions),
}

impl StdbAuthSource {
    pub fn client_id(&self) -> Option<String> {
        match self {
            #[cfg(feature = "auth-oidc")]
            StdbAuthSource::Oidc(opts) => Some(opts.client_id.clone()),
            #[cfg(feature = "auth-steam")]
            StdbAuthSource::Steam(opts) => Some(opts.client_id.clone()),
        }
    }

    #[cfg(feature = "auth-oidc")]
    pub(crate) fn post_logout_redirect_uri(&self) -> Option<String> {
        match self {
            StdbAuthSource::Oidc(opts) => opts.post_logout_redirect_uri.clone(),
            #[cfg(feature = "auth-steam")]
            StdbAuthSource::Steam(_) => None,
        }
    }

    pub(crate) async fn acquire_token_response(&self) -> Result<StdbTokenResponse, StdbAuthError> {
        match self {
            #[cfg(feature = "auth-oidc")]
            StdbAuthSource::Oidc(opts) => oidc::acquire_token_response(opts).await,
            #[cfg(feature = "auth-steam")]
            StdbAuthSource::Steam(opts) => steam::acquire_token_response(opts),
        }
    }
}

/// Stores the token payload returned by the token endpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Resource, serde::Deserialize)]
pub(crate) struct StdbTokenResponse {
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
    /// Optional ID token returned by the OIDC provider.
    pub id_token: Option<String>,
    /// Optional URI returned to after browser logout.
    #[serde(skip, default)]
    pub post_logout_redirect_uri: Option<String>,
}

// TODO:
// persist refresh token to secure storage when the bevy app shuts down... how to do this?

// curl -X POST https://auth.spacetimedb.com/oidc/token \
//   -H "content-type: application/x-www-form-urlencoded" \
//   -d "grant_type=refresh_token" \
//   -d "refresh_token=<REFRESH_TOKEN>" \
//   -d "client_id=<CLIENT_ID>"

pub(crate) mod error;
pub(crate) mod plugin;

#[cfg(feature = "auth-oidc")]
pub(crate) mod oidc;
#[cfg(feature = "auth-steam")]
pub(crate) mod steam;

pub(crate) use error::StdbAuthError;
pub use plugin::StdbAuthPlugin;

#[cfg(feature = "auth-oidc")]
pub use oidc::{StdbOidcAuthOptions, StdbOidcPrompt};
#[cfg(feature = "auth-steam")]
pub use steam::StdbSteamAuthOptions;

use bevy_ecs::prelude::Resource;

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
    /// The ID token returned by the OIDC provider.
    pub id_token: Option<String>,
}

// TODO:
// persist refresh token to secure storage when the bevy app shuts down... how to do this?

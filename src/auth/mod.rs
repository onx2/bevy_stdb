// curl -X POST https://auth.spacetimedb.com/oidc/token \
//   -H "content-type: application/x-www-form-urlencoded" \
//   -d "grant_type=refresh_token" \
//   -d "refresh_token=<REFRESH_TOKEN>" \
//   -d "client_id=<CLIENT_ID>"

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub(crate) mod error;
#[cfg(feature = "auth-oidc")]
pub(crate) mod oidc;
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub(crate) mod plugin;
#[cfg(feature = "auth-steam")]
pub(crate) mod steam;

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub(crate) use error::StdbAuthError;
#[cfg(feature = "auth-oidc")]
pub use oidc::{StdbOidcAuthOptions, StdbOidcPrompt};
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub use plugin::StdbAuthPlugin;

#[cfg(feature = "auth-steam")]
pub use steam::StdbSteamAuthOptions;

use bevy_ecs::prelude::Resource;

/// The specific auth target for a given attempt.
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
#[derive(Clone, Debug)]
pub enum StdbAuthSource {
    #[cfg(feature = "auth-oidc")]
    Oidc(StdbOidcAuthOptions),
    #[cfg(all(feature = "auth-steam", not(feature = "browser")))]
    Steam(StdbSteamAuthOptions),
}

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
impl StdbAuthSource {
    pub fn client_id(&self) -> Option<String> {
        match self {
            #[cfg(feature = "auth-oidc")]
            StdbAuthSource::Oidc(opts) => Some(opts.client_id.clone()),
            #[cfg(all(feature = "auth-steam", not(feature = "browser")))]
            StdbAuthSource::Steam(opts) => Some(opts.client_id.clone()),
        }
    }

    pub(crate) async fn acquire_token_response(&self) -> Result<StdbTokenResponse, StdbAuthError> {
        match self {
            #[cfg(feature = "auth-oidc")]
            StdbAuthSource::Oidc(opts) => oidc::acquire_token_response(opts).await,
            #[cfg(all(feature = "auth-steam", not(feature = "browser")))]
            StdbAuthSource::Steam(opts) => steam::acquire_token_response(opts),
        }
    }
}

/// Stores the token payload returned by the token endpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Resource)]
#[cfg_attr(
    any(feature = "auth-oidc", feature = "auth-steam"),
    derive(serde::Deserialize)
)]
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
}

// TODO:
// persist refresh token to secure storage when the bevy app shuts down... how to do this?

// curl -X POST https://auth.spacetimedb.com/oidc/token \
//   -H "content-type: application/x-www-form-urlencoded" \
//   -d "grant_type=refresh_token" \
//   -d "refresh_token=<REFRESH_TOKEN>" \
//   -d "client_id=<CLIENT_ID>"

#[cfg(feature = "auth-oidc")]
pub(crate) mod oidc;
#[cfg(feature = "auth-steam")]
pub(crate) mod steam;

#[cfg(feature = "auth-oidc")]
pub use oidc::StdbOidcAuthOptions;
#[cfg(feature = "auth-oidc")]
pub(crate) use oidc::StdbOidcAuthPlugin;

#[cfg(feature = "auth-steam")]
pub use steam::StdbSteamAuthOptions;
#[cfg(feature = "auth-steam")]
pub(crate) use steam::StdbSteamAuthPlugin;

/// The specific auth target for a given attempt
#[derive(Clone, Debug)]
pub enum StdbAuthTarget {
    Token(String),
    #[cfg(feature = "auth-oidc")]
    Oidc(StdbOidcAuthOptions),
    #[cfg(feature = "auth-steam")]
    Steam(StdbSteamAuthOptions),
}

impl StdbAuthTarget {
    pub(crate) fn acquire_token_response(&self) -> Option<TokenResponse> {
        match self {
            #[cfg(feature = "auth-oidc")]
            StdbAuthTarget::Oidc(opts) => oidc::acquire_token_response(opts).ok(), // TODO handle error
            #[cfg(feature = "auth-steam")]
            StdbAuthTarget::Steam(opts) => steam::acquire_token_response(opts).ok(), // TODO handle error
            StdbAuthTarget::Token(token) => Some(TokenResponse {
                access_token: token.to_owned(),
                ..TokenResponse::default()
            }),
        }
    }
}

/// Stores the token payload returned by the token endpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    any(feature = "auth-oidc", feature = "auth-steam"),
    derive(serde::Deserialize)
)]
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
}

// TODO:
// persist refresh token to secure storage when the bevy app shuts down... how to do this?

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub(crate) mod error;
#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub(crate) use error::StdbAuthError;

// curl -X POST https://auth.spacetimedb.com/oidc/token \
//   -H "content-type: application/x-www-form-urlencoded" \
//   -d "grant_type=refresh_token" \
//   -d "refresh_token=<REFRESH_TOKEN>" \
//   -d "client_id=<CLIENT_ID>"

// /// The authorization endpoint.
// pub auth_endpoint: String,
// /// The token endpoint.
// pub token_endpoint: String,
// /// The token endpoint.
// pub token_endpoint: String,
// /// The identity of the web service that accepts Steam tickets
// /// For example, "spacetimeauth" when using SpacetimeAuth
// pub ticket_identity: String,

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod auth_imp;

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod auth_imp;

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
mod plugin;

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub use plugin::StdbAuthOptions;

#[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
pub(crate) use plugin::StdbAuthPlugin;

/// The specific auth target for a given attempt
#[derive(Clone, Debug)]
pub enum StdbAuthTarget {
    Token(String),
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
    /// The unique identifier for your Steam game.
    pub app_id: usize,
}

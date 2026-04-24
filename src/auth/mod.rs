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

// #[cfg(any(feature = "auth-oidc", feature = "auth-steam"))]
// mod plugin;

#[cfg(feature = "auth-oidc")]
mod oidc;
#[cfg(feature = "auth-steam")]
mod steam;

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

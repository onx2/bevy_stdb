#[cfg(feature = "browser")]
#[path = "web.rs"]
mod auth_imp;

#[cfg(not(feature = "browser"))]
#[path = "native.rs"]
mod auth_imp;

use super::{StdbAuthError, StdbTokenResponse};

#[derive(Clone, Debug)]
pub struct StdbOidcAuthOptions {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The redirect URI used by the client.
    pub redirect_uri: String,
    /// The requested scopes.
    pub scopes: Vec<String>,
}

pub async fn acquire_token_response(
    options: &StdbOidcAuthOptions,
) -> Result<StdbTokenResponse, StdbAuthError> {
    auth_imp::acquire_token_response(options).await
}

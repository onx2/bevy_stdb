#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod auth_imp;

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod auth_imp;

use super::{StdbAuthError, TokenResponse};
pub use auth_imp::acquire_token_response;

#[derive(Clone, Debug)]
pub struct StdbOidcAuthOptions {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The redirect URI used by the client.
    pub redirect_uri: String,
    /// The requested scopes.
    pub scopes: Vec<String>,
}

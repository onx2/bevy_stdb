mod common;
#[cfg(not(feature = "browser"))]
pub(crate) mod keyring;

#[cfg(feature = "browser")]
#[path = "web.rs"]
mod auth_imp;

#[cfg(not(feature = "browser"))]
#[path = "native.rs"]
mod auth_imp;

use super::{StdbAuthError, StdbTokenResponse};

/// Controls the OIDC `prompt` authorization parameter.
#[derive(Clone, Debug, Default)]
pub enum StdbOidcPrompt {
    /// Allows the provider to decide whether user interaction is required.
    #[default]
    None,
    /// Requests that the provider force user authentication.
    Login,
    /// Requests that the provider prompt for account selection.
    SelectAccount,
}

impl StdbOidcPrompt {
    /// Returns the OIDC `prompt` parameter value.
    pub(crate) fn as_param(&self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Login => Some("login"),
            Self::SelectAccount => Some("select_account"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StdbOidcAuthOptions {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The redirect URI used by the client.
    pub redirect_uri: String,
    /// The requested scopes.
    pub scopes: Vec<String>,
    /// The prompt behavior for interactive authorization.
    pub prompt: StdbOidcPrompt,
}

pub async fn acquire_token_response(
    options: &StdbOidcAuthOptions,
) -> Result<StdbTokenResponse, StdbAuthError> {
    auth_imp::acquire_token_response(options)
}

use super::{StdbAuthError, StdbOidcAuthOptions, StdbTokenResponse};
use crate::log::error;

/// Acquires a token response using the browser OIDC flow.
pub async fn acquire_token_response(
    _options: &StdbOidcAuthOptions,
) -> Result<StdbTokenResponse, StdbAuthError> {
    error!("browser OIDC authentication is not implemented yet");

    Err(StdbAuthError::Internal(
        "browser OIDC authentication is not implemented yet".to_string(),
    ))
}

/// Stub for OIDC session termination in the browser.
pub(crate) async fn end_session(
    _client_id: Option<&str>,
    _id_token: &str,
) -> Result<(), StdbAuthError> {
    error!("browser OIDC session end is not implemented yet");
    Err(StdbAuthError::Internal(
        "browser OIDC session end is not implemented yet".to_string(),
    ))
}

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

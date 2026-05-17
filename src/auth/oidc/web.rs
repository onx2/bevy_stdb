use super::super::AUTH_URI_BASE;
use super::{StdbAuthError, StdbOidcAuthOptions, StdbTokenResponse};
use bevy_log::{error, info, warn};
use oauth2::PkceCodeVerifier;
use serde::{Deserialize, Serialize};
use url::Url;
use web_sys::wasm_bindgen::JsValue;
use web_sys::{Storage, Window};

use super::common::{authorization_request, query_param};

const PENDING_CONTEXT_KEY: &str = "bevy_stdb.auth.oidc.pending";

/// Describes the result of resuming a browser OIDC callback.
#[derive(Debug)]
pub(crate) enum WebOidcCallbackOutcome {
    /// No OIDC callback is present in the current browser URL.
    NoCallback,
    /// The current browser URL contains an OIDC failure response.
    Failure { message: String },
    /// The callback was exchanged for a token response.
    Success {
        token_response: StdbTokenResponse,
        client_id: String,
    },
}

/// Returns whether the current browser URL contains OIDC callback parameters.
pub(crate) fn browser_oidc_callback_is_present() -> Result<bool, StdbAuthError> {
    let current_url = current_url()?;
    Ok(query_param(&current_url, "code").is_some()
        || query_param(&current_url, "state").is_some()
        || query_param(&current_url, "error").is_some())
}

/// Attempts to resume an OIDC login from the current browser URL.
pub(crate) async fn try_resume_token_response_from_callback()
-> Result<WebOidcCallbackOutcome, StdbAuthError> {
    let current_url = current_url()?;

    if let Some(error) = query_param(&current_url, "error") {
        let error_description = query_param(&current_url, "error_description");
        return callback_failure(format_oauth_error(error, error_description), true);
    }

    let has_code = query_param(&current_url, "code").is_some();
    let has_state = query_param(&current_url, "state").is_some();

    match (has_code, has_state) {
        (false, false) => return Ok(WebOidcCallbackOutcome::NoCallback),
        (false, true) => {
            return callback_failure(
                "OIDC callback is missing the authorization code".to_string(),
                true,
            );
        }
        (true, false) => {
            return callback_failure("OIDC callback is missing the state".to_string(), true);
        }
        (true, true) => {}
    }

    let code = query_param(&current_url, "code").ok_or_else(|| {
        StdbAuthError::Internal("OIDC callback is missing the authorization code".to_string())
    })?;
    let state = query_param(&current_url, "state")
        .ok_or_else(|| StdbAuthError::Internal("OIDC callback is missing the state".to_string()))?;
    let pending_context = match load_pending_context() {
        Ok(pending_context) => pending_context,
        Err(error) => {
            clear_callback_query()?;
            return Err(error);
        }
    };

    if state != pending_context.state {
        return callback_failure(
            "OIDC callback state did not match the stored CSRF token".to_string(),
            true,
        );
    }

    clear_pending_context()?;
    clear_callback_query()?;

    let client_id = pending_context.client_id.clone();
    let token_response = exchange_code_for_token(code, pending_context).await?;

    Ok(WebOidcCallbackOutcome::Success {
        token_response,
        client_id,
    })
}

/// Acquires a token response using the browser OIDC flow.
pub async fn acquire_token_response(
    options: &StdbOidcAuthOptions,
) -> Result<StdbTokenResponse, StdbAuthError> {
    begin_login(options)?;
    std::future::pending().await
}

#[derive(Debug, Deserialize, Serialize)]
struct PendingOidcContext {
    client_id: String,
    redirect_uri: String,
    state: String,
    code_verifier: String,
}

fn begin_login(options: &StdbOidcAuthOptions) -> Result<(), StdbAuthError> {
    info!(
        "starting browser OIDC authentication with client_id={} and redirect_uri={}",
        options.client_id, options.redirect_uri
    );

    let authorization_request = authorization_request(options).map_err(|error| {
        error!("failed to create OIDC authorization request: {error:?}");
        error
    })?;

    store_pending_context(&PendingOidcContext {
        client_id: options.client_id.clone(),
        redirect_uri: options.redirect_uri.clone(),
        state: authorization_request.csrf_token.secret().to_string(),
        code_verifier: authorization_request.pkce_verifier.secret().to_string(),
    })?;

    info!("redirecting to OIDC authorization URL");

    window()?
        .location()
        .set_href(authorization_request.auth_url.as_str())
        .map_err(js_auth_error)?;

    Ok(())
}

async fn exchange_code_for_token(
    code: String,
    pending_context: PendingOidcContext,
) -> Result<StdbTokenResponse, StdbAuthError> {
    let code_verifier = PkceCodeVerifier::new(pending_context.code_verifier);
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{AUTH_URI_BASE}/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", pending_context.redirect_uri.as_str()),
            ("client_id", pending_context.client_id.as_str()),
            ("code_verifier", code_verifier.secret()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<StdbTokenResponse>()
        .await?;

    info!(
        "browser OIDC authentication succeeded; received access token with expires_in={:?}, refresh_token_present={}",
        response.expires_in,
        response.refresh_token.is_some()
    );

    Ok(response)
}

fn callback_failure(
    message: String,
    clear_context: bool,
) -> Result<WebOidcCallbackOutcome, StdbAuthError> {
    warn!("browser OIDC callback failed: {message}");

    if clear_context {
        clear_pending_context()?;
    }
    clear_callback_query()?;

    Ok(WebOidcCallbackOutcome::Failure { message })
}

fn current_url() -> Result<Url, StdbAuthError> {
    let href = window()?.location().href().map_err(js_auth_error)?;
    Url::parse(&href).map_err(|error| {
        error!("invalid browser URL: {error}");
        StdbAuthError::Internal(format!("invalid browser URL: {error}"))
    })
}

fn clear_callback_query() -> Result<(), StdbAuthError> {
    let mut url = current_url()?;
    if url.query().is_none() && url.fragment().is_none() {
        return Ok(());
    }

    url.set_query(None);
    url.set_fragment(None);

    window()?
        .history()
        .map_err(js_auth_error)?
        .replace_state_with_url(&JsValue::NULL, "", Some(url.as_str()))
        .map_err(js_auth_error)
}

fn store_pending_context(context: &PendingOidcContext) -> Result<(), StdbAuthError> {
    let value = serde_json::to_string(context)?;
    session_storage()?
        .set_item(PENDING_CONTEXT_KEY, &value)
        .map_err(js_auth_error)
}

fn load_pending_context() -> Result<PendingOidcContext, StdbAuthError> {
    let value = session_storage()?
        .get_item(PENDING_CONTEXT_KEY)
        .map_err(js_auth_error)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            StdbAuthError::Internal("OIDC callback is missing stored browser state".to_string())
        })?;

    serde_json::from_str(&value).map_err(StdbAuthError::Decode)
}

fn clear_pending_context() -> Result<(), StdbAuthError> {
    session_storage()?
        .remove_item(PENDING_CONTEXT_KEY)
        .map_err(js_auth_error)
}

fn session_storage() -> Result<Storage, StdbAuthError> {
    window()?
        .session_storage()
        .map_err(js_auth_error)?
        .ok_or_else(|| {
            StdbAuthError::Internal("browser session storage is unavailable".to_string())
        })
}

fn window() -> Result<Window, StdbAuthError> {
    web_sys::window()
        .ok_or_else(|| StdbAuthError::Internal("browser window is unavailable".to_string()))
}

fn format_oauth_error(error: String, error_description: Option<String>) -> String {
    match error_description {
        Some(description) => format!("OAuth error: {error} ({description})"),
        None => format!("OAuth error: {error}"),
    }
}

fn js_auth_error(value: JsValue) -> StdbAuthError {
    StdbAuthError::Internal(
        value
            .as_string()
            .unwrap_or_else(|| "JavaScript operation failed".to_string()),
    )
}

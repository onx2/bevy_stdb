#![cfg(all(feature = "browser", target_arch = "wasm32"))]

use super::{CurrentAuthTokens, PendingAuthMode, StdbAuthOptions, StdbTokenStorage, TokenResponse};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use wasm_bindgen_futures::spawn_local;
use web_sys::wasm_bindgen::JsValue;
use web_sys::{Storage, Window};

const STORAGE_STATE: &str = "bevy_stdb.auth.state";
const STORAGE_CODE_VERIFIER: &str = "bevy_stdb.auth.code_verifier";
const STORAGE_REFRESH_TOKEN: &str = "bevy_stdb.auth.refresh_token";

thread_local! {
    static PENDING_WEB_AUTH_RESULT: RefCell<PendingWebAuthResultState> =
        const { RefCell::new(PendingWebAuthResultState::Idle) };
}

#[derive(Debug)]
pub(super) enum WebAuthError {
    MissingWindow,
    MissingSessionStorage,
    MissingLocalStorage,
    Js(String),
    InvalidUrl(url::ParseError),
    MissingQueryParam(&'static str),
    MissingStoredValue(&'static str),
    StateMismatch,
    Http(reqwest::Error),
    Json(reqwest::Error),
    Storage(String),
    MissingRefreshToken,
}

impl std::fmt::Display for WebAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingWindow => write!(f, "browser window is unavailable"),
            Self::MissingSessionStorage => write!(f, "browser session storage is unavailable"),
            Self::MissingLocalStorage => write!(f, "browser local storage is unavailable"),
            Self::Js(message) => write!(f, "browser error: {message}"),
            Self::InvalidUrl(error) => write!(f, "invalid URL: {error}"),
            Self::MissingQueryParam(param) => write!(f, "missing query parameter: {param}"),
            Self::MissingStoredValue(key) => write!(f, "missing stored value: {key}"),
            Self::StateMismatch => write!(f, "OIDC state mismatch"),
            Self::Http(error) => write!(f, "HTTP error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::Storage(message) => write!(f, "storage error: {message}"),
            Self::MissingRefreshToken => write!(f, "missing refresh token"),
        }
    }
}

impl std::error::Error for WebAuthError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WebAuthCallbackStatus {
    None,
    Failure { message: String },
    Ready,
}

#[derive(Debug)]
struct AuthorizationCallback {
    code: String,
    state: String,
}

#[derive(Debug)]
struct CallbackExchange {
    code: String,
    code_verifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredRefreshToken {
    refresh_token: String,
}

#[derive(Debug, Clone)]
enum PendingWebAuthResultState {
    Idle,
    Pending,
    Ready(Result<TokenResponse, String>),
}

#[derive(Debug, Deserialize)]
struct RawTokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

impl From<RawTokenResponse> for TokenResponse {
    fn from(value: RawTokenResponse) -> Self {
        Self {
            access_token: value.access_token,
            expires_in: value.expires_in.unwrap_or_default(),
            refresh_token: value.refresh_token,
            scope: value.scope,
            token_type: value.token_type.unwrap_or_default(),
            id_token: value.id_token,
        }
    }
}

pub(super) fn callback_status() -> Result<WebAuthCallbackStatus, WebAuthError> {
    let query_pairs = query_pairs()?;

    let error = query_value(&query_pairs, "error");
    let error_description = query_value(&query_pairs, "error_description");
    if let Some(error) = error {
        println!("bevy_stdb web auth: callback contained oauth error");
        return Ok(WebAuthCallbackStatus::Failure {
            message: format_oauth_error(error, error_description),
        });
    }

    let has_code = query_value(&query_pairs, "code").is_some();
    let has_state = query_value(&query_pairs, "state").is_some();

    println!(
        "bevy_stdb web auth: evaluated callback status; has_code={}, has_state={}",
        has_code, has_state
    );

    match (has_code, has_state) {
        (false, false) => Ok(WebAuthCallbackStatus::None),
        (true, true) => Ok(WebAuthCallbackStatus::Ready),
        (false, true) => Ok(WebAuthCallbackStatus::Failure {
            message: WebAuthError::MissingQueryParam("code").to_string(),
        }),
        (true, false) => Ok(WebAuthCallbackStatus::Failure {
            message: WebAuthError::MissingQueryParam("state").to_string(),
        }),
    }
}

pub(super) fn try_resolve_auth_now(
    options: &StdbAuthOptions,
    mode: PendingAuthMode,
    current_tokens: &CurrentAuthTokens,
) -> Option<Result<TokenResponse, String>> {
    if let Some(result) = take_pending_result() {
        return Some(result);
    }

    if web_auth_result_is_pending() {
        return None;
    }

    match mode {
        PendingAuthMode::Interactive => {
            println!("bevy_stdb web auth: starting interactive login flow");
            match begin_interactive_login(options) {
                Ok(()) => None,
                Err(error) => Some(Err(error)),
            }
        }
        PendingAuthMode::Silent | PendingAuthMode::Reconnect => {
            println!(
                "bevy_stdb web auth: attempting silent refresh; has_in_memory_refresh={}",
                current_tokens.refresh_token().is_some()
            );

            begin_refresh_exchange(options.clone(), current_tokens);
            None
        }
    }
}

pub(super) fn begin_interactive_login(options: &StdbAuthOptions) -> Result<(), String> {
    let state = random_url_safe_string(32).map_err(|error| error.to_string())?;
    let code_verifier = random_url_safe_string(64).map_err(|error| error.to_string())?;
    let code_challenge = pkce_challenge_sha256(&code_verifier);

    println!(
        "bevy_stdb web auth: preparing interactive login request; redirect_uri={}, auth_endpoint={}, scope_count={}",
        options.redirect_uri,
        options.auth_endpoint,
        options.scopes.len()
    );

    store_session(STORAGE_STATE, &state).map_err(|error| error.to_string())?;
    store_session(STORAGE_CODE_VERIFIER, &code_verifier).map_err(|error| error.to_string())?;

    let mut auth_url = url::Url::parse(&options.auth_endpoint)
        .map_err(WebAuthError::InvalidUrl)
        .map_err(|error| error.to_string())?;

    {
        let mut query = auth_url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", &options.client_id);
        query.append_pair("redirect_uri", &options.redirect_uri);
        query.append_pair("code_challenge_method", "S256");
        query.append_pair("code_challenge", &code_challenge);
        query.append_pair("state", &state);

        if !options.scopes.is_empty() {
            query.append_pair("scope", &options.scopes.join(" "));
        }
    }

    println!("bevy_stdb web auth: redirecting browser to authorization endpoint");

    window()
        .and_then(|window| {
            window
                .location()
                .set_href(auth_url.as_str())
                .map_err(js_error)
        })
        .map_err(|error| error.to_string())
}

pub(super) fn has_auth_callback() -> bool {
    callback_status()
        .map(|status| status != WebAuthCallbackStatus::None)
        .unwrap_or(false)
}

pub(super) fn begin_callback_exchange(options: StdbAuthOptions) -> Result<(), String> {
    println!("bevy_stdb web auth: beginning non-blocking callback exchange");

    let exchange = capture_callback_exchange().map_err(|error| error.to_string())?;
    set_pending_marker();

    spawn_local(async move {
        let result = resume_callback(options, exchange).await;
        set_pending_result(result);
    });

    Ok(())
}

pub(super) fn begin_refresh_exchange(options: StdbAuthOptions, current_tokens: &CurrentAuthTokens) {
    let in_memory_refresh_token = current_tokens.refresh_token().map(ToOwned::to_owned);
    let stored_refresh_token = if in_memory_refresh_token.is_none() {
        load_refresh_token(&options.storage).ok()
    } else {
        None
    };

    println!(
        "bevy_stdb web auth: resolving refresh token source; has_in_memory_refresh={}, has_stored_refresh={}",
        in_memory_refresh_token.is_some(),
        stored_refresh_token.is_some()
    );

    let refresh_token = match in_memory_refresh_token.or(stored_refresh_token) {
        Some(refresh_token) => refresh_token,
        None => {
            set_pending_result(Err(WebAuthError::MissingRefreshToken.to_string()));
            return;
        }
    };

    set_pending_marker();

    spawn_local(async move {
        println!("bevy_stdb web auth: requesting new access token from refresh token");

        let result = async {
            let mut tokens = refresh_token_request(&options, &refresh_token)
                .await
                .map_err(|error| error.to_string())?;

            if tokens.refresh_token.is_none() {
                println!(
                    "bevy_stdb web auth: refresh response did not include refresh token, preserving previous token"
                );
                tokens.refresh_token = Some(refresh_token);
            } else {
                println!("bevy_stdb web auth: refresh response included a refresh token");
            }

            persist_refresh_token(&options.storage, tokens.refresh_token.as_deref())
                .map_err(|error| error.to_string())?;

            println!(
                "bevy_stdb web auth: refresh exchange completed; has_refresh_token={}, expires_in={}",
                tokens.refresh_token.is_some(),
                tokens.expires_in
            );

            Ok(tokens)
        }
        .await;

        set_pending_result(result);
    });
}

pub(super) fn take_pending_result() -> Option<Result<TokenResponse, String>> {
    PENDING_WEB_AUTH_RESULT.with(|slot| {
        let mut slot = slot.borrow_mut();
        match std::mem::replace(&mut *slot, PendingWebAuthResultState::Idle) {
            PendingWebAuthResultState::Ready(result) => Some(result),
            PendingWebAuthResultState::Pending => {
                *slot = PendingWebAuthResultState::Pending;
                None
            }
            PendingWebAuthResultState::Idle => None,
        }
    })
}

pub(super) fn web_auth_result_is_pending() -> bool {
    PENDING_WEB_AUTH_RESULT
        .with(|slot| matches!(*slot.borrow(), PendingWebAuthResultState::Pending))
}

pub(super) fn store_tokens(
    options: &StdbAuthOptions,
    tokens: &TokenResponse,
) -> Result<(), String> {
    persist_refresh_token(&options.storage, tokens.refresh_token.as_deref())
        .map_err(|error| error.to_string())
}

pub(super) fn clear_stored_tokens(options: &StdbAuthOptions) -> Result<(), String> {
    println!("bevy_stdb web auth: clearing stored tokens and pkce session state");
    let _ = clear_pkce_storage();
    let _ = clear_callback_query();
    persist_refresh_token(&options.storage, None).map_err(|error| error.to_string())
}

pub(super) fn clear_callback_query() -> Result<(), WebAuthError> {
    let current_search = window()?.location().search().map_err(js_error)?;
    if current_search.is_empty() {
        return Ok(());
    }

    window()?
        .history()
        .map_err(js_error)?
        .replace_state_with_url(&JsValue::NULL, "", Some(&current_origin_path()?))
        .map_err(js_error)?;

    Ok(())
}

pub(super) fn load_refresh_token(storage: &StdbTokenStorage) -> Result<String, WebAuthError> {
    match storage {
        StdbTokenStorage::None => {
            println!("bevy_stdb web auth: refresh token storage disabled");
            Err(WebAuthError::MissingRefreshToken)
        }
        StdbTokenStorage::PlatformDefault => {
            let value = local_storage()?
                .get_item(STORAGE_REFRESH_TOKEN)
                .map_err(js_error)?
                .ok_or_else(|| {
                    println!("bevy_stdb web auth: did not find refresh token in local storage");
                    WebAuthError::MissingRefreshToken
                })?;

            let stored: StoredRefreshToken = serde_json::from_str(&value)
                .map_err(|error| WebAuthError::Storage(error.to_string()))?;

            if stored.refresh_token.trim().is_empty() {
                println!("bevy_stdb web auth: found blank refresh token in local storage");
                return Err(WebAuthError::MissingRefreshToken);
            }

            println!("bevy_stdb web auth: loaded refresh token from local storage");
            Ok(stored.refresh_token)
        }
    }
}

pub(super) fn persist_refresh_token(
    storage: &StdbTokenStorage,
    refresh_token: Option<&str>,
) -> Result<(), WebAuthError> {
    match storage {
        StdbTokenStorage::None => {
            println!(
                "bevy_stdb web auth: skipped refresh token persistence because storage is disabled"
            );
            Ok(())
        }
        StdbTokenStorage::PlatformDefault => {
            let storage = local_storage()?;

            match refresh_token {
                Some(refresh_token) if !refresh_token.trim().is_empty() => {
                    println!("bevy_stdb web auth: persisting refresh token to local storage");
                    let stored = StoredRefreshToken {
                        refresh_token: refresh_token.to_string(),
                    };
                    let value = serde_json::to_string(&stored)
                        .map_err(|error| WebAuthError::Storage(error.to_string()))?;
                    storage
                        .set_item(STORAGE_REFRESH_TOKEN, &value)
                        .map_err(js_error)?;
                }
                _ => {
                    println!("bevy_stdb web auth: removing refresh token from local storage");
                    storage
                        .remove_item(STORAGE_REFRESH_TOKEN)
                        .map_err(js_error)?;
                }
            }

            Ok(())
        }
    }
}

fn set_pending_marker() {
    PENDING_WEB_AUTH_RESULT.with(|slot| {
        *slot.borrow_mut() = PendingWebAuthResultState::Pending;
    });
}

fn set_pending_result(result: Result<TokenResponse, String>) {
    PENDING_WEB_AUTH_RESULT.with(|slot| {
        *slot.borrow_mut() = PendingWebAuthResultState::Ready(result);
    });
}

fn capture_callback_exchange() -> Result<CallbackExchange, WebAuthError> {
    let callback = parse_authorization_callback()?;
    let expected_state = load_session(STORAGE_STATE)?;
    let code_verifier = load_session(STORAGE_CODE_VERIFIER)?;

    println!("bevy_stdb web auth: validating callback state against session storage");

    if callback.state != expected_state {
        println!("bevy_stdb web auth: callback state mismatch");
        let _ = clear_pkce_storage();
        let _ = clear_callback_query();
        return Err(WebAuthError::StateMismatch);
    }

    let _ = clear_pkce_storage();
    let _ = clear_callback_query();

    Ok(CallbackExchange {
        code: callback.code,
        code_verifier,
    })
}

fn parse_authorization_callback() -> Result<AuthorizationCallback, WebAuthError> {
    let query_pairs = query_pairs()?;

    if let Some(error) = query_value(&query_pairs, "error") {
        let error_description = query_value(&query_pairs, "error_description");
        return Err(WebAuthError::Storage(format_oauth_error(
            error,
            error_description,
        )));
    }

    let code = query_value(&query_pairs, "code").ok_or(WebAuthError::MissingQueryParam("code"))?;
    let state =
        query_value(&query_pairs, "state").ok_or(WebAuthError::MissingQueryParam("state"))?;

    Ok(AuthorizationCallback { code, state })
}

async fn resume_callback(
    options: StdbAuthOptions,
    exchange: CallbackExchange,
) -> Result<TokenResponse, String> {
    println!("bevy_stdb web auth: exchanging authorization code for tokens");

    let mut tokens = exchange_code_for_token(&options, exchange)
        .await
        .map_err(|error| error.to_string())?;

    if tokens.refresh_token.is_none() {
        println!(
            "bevy_stdb web auth: callback response did not include refresh token, checking storage"
        );
        tokens.refresh_token = load_refresh_token(&options.storage).ok();
    } else {
        println!("bevy_stdb web auth: callback response included refresh token");
    }

    persist_refresh_token(&options.storage, tokens.refresh_token.as_deref())
        .map_err(|error| error.to_string())?;

    println!(
        "bevy_stdb web auth: callback exchange completed; has_refresh_token={}, expires_in={}",
        tokens.refresh_token.is_some(),
        tokens.expires_in
    );

    Ok(tokens)
}

async fn exchange_code_for_token(
    options: &StdbAuthOptions,
    exchange: CallbackExchange,
) -> Result<TokenResponse, WebAuthError> {
    let http_client = Client::builder().build().map_err(WebAuthError::Http)?;

    println!(
        "bevy_stdb web auth: posting authorization_code token request; token_endpoint={}, redirect_uri={}",
        options.token_endpoint, options.redirect_uri
    );

    let response = http_client
        .post(&options.token_endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", exchange.code.as_str()),
            ("client_id", options.client_id.as_str()),
            ("redirect_uri", options.redirect_uri.as_str()),
            ("code_verifier", exchange.code_verifier.as_str()),
        ])
        .send()
        .await
        .map_err(WebAuthError::Http)?
        .error_for_status()
        .map_err(WebAuthError::Http)?;

    let token_response = response
        .json::<RawTokenResponse>()
        .await
        .map_err(WebAuthError::Json)?;

    Ok(token_response.into())
}

async fn refresh_token_request(
    options: &StdbAuthOptions,
    refresh_token: &str,
) -> Result<TokenResponse, WebAuthError> {
    let http_client = Client::builder().build().map_err(WebAuthError::Http)?;

    println!(
        "bevy_stdb web auth: posting refresh_token request; token_endpoint={}, refresh_token_present={}",
        options.token_endpoint,
        !refresh_token.trim().is_empty()
    );

    let response = http_client
        .post(&options.token_endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", options.client_id.as_str()),
        ])
        .send()
        .await
        .map_err(WebAuthError::Http)?
        .error_for_status()
        .map_err(WebAuthError::Http)?;

    let token_response = response
        .json::<RawTokenResponse>()
        .await
        .map_err(WebAuthError::Json)?;

    Ok(token_response.into())
}

fn query_pairs() -> Result<Vec<(String, String)>, WebAuthError> {
    let search = window()?.location().search().map_err(js_error)?;
    if search.is_empty() {
        return Ok(Vec::new());
    }

    Ok(
        url::form_urlencoded::parse(search.trim_start_matches('?').as_bytes())
            .into_owned()
            .collect(),
    )
}

fn query_value(query_pairs: &[(String, String)], key: &str) -> Option<String> {
    query_pairs
        .iter()
        .find(|(current_key, _)| current_key == key)
        .map(|(_, value)| value.clone())
}

fn store_session(key: &'static str, value: &str) -> Result<(), WebAuthError> {
    session_storage()?.set_item(key, value).map_err(js_error)
}

fn load_session(key: &'static str) -> Result<String, WebAuthError> {
    session_storage()?
        .get_item(key)
        .map_err(js_error)?
        .filter(|value| !value.trim().is_empty())
        .ok_or(WebAuthError::MissingStoredValue(key))
}

fn clear_pkce_storage() -> Result<(), WebAuthError> {
    let storage = session_storage()?;
    storage.remove_item(STORAGE_STATE).map_err(js_error)?;
    storage
        .remove_item(STORAGE_CODE_VERIFIER)
        .map_err(js_error)?;
    Ok(())
}

fn session_storage() -> Result<Storage, WebAuthError> {
    window()?
        .session_storage()
        .map_err(js_error)?
        .ok_or(WebAuthError::MissingSessionStorage)
}

fn local_storage() -> Result<Storage, WebAuthError> {
    window()?
        .local_storage()
        .map_err(js_error)?
        .ok_or(WebAuthError::MissingLocalStorage)
}

fn current_origin_path() -> Result<String, WebAuthError> {
    let location = window()?.location();
    let origin = location.origin().map_err(js_error)?;
    let pathname = location.pathname().map_err(js_error)?;
    Ok(format!("{origin}{pathname}"))
}

fn window() -> Result<Window, WebAuthError> {
    web_sys::window().ok_or(WebAuthError::MissingWindow)
}

fn random_url_safe_string(byte_len: usize) -> Result<String, WebAuthError> {
    let mut bytes = vec![0_u8; byte_len];
    window()?
        .crypto()
        .map_err(js_error)?
        .get_random_values_with_u8_array(&mut bytes)
        .map_err(js_error)?;

    Ok(base64_url_no_pad(&bytes))
}

fn pkce_challenge_sha256(verifier: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(verifier.as_bytes());
    base64_url_no_pad(&digest)
}

fn base64_url_no_pad(bytes: &[u8]) -> String {
    use base64::Engine as _;

    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn format_oauth_error(error: String, error_description: Option<String>) -> String {
    match error_description {
        Some(description) => format!("OAuth error: {error} ({description})"),
        None => format!("OAuth error: {error}"),
    }
}

fn js_error(value: JsValue) -> WebAuthError {
    WebAuthError::Js(
        value
            .as_string()
            .unwrap_or_else(|| "JavaScript operation failed".to_string()),
    )
}

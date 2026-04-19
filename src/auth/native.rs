#![cfg(all(not(feature = "browser"), not(target_arch = "wasm32")))]
//! Native OIDC helpers for desktop authentication and refresh flows.

use super::{
    CurrentAuthTokens, PendingAuthMode, StdbAuthConfig, StdbAuthOptions, StdbTokenStorage,
    TokenResponse,
};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse as OAuthTokenResponse,
    TokenUrl,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use url::Url;

const STORAGE_DIR_NAME: &str = "bevy_stdb";
const STORAGE_FILE_PREFIX: &str = "stdb_oidc";

const LOGIN_SUCCESS_BODY: &str = r#"<html style="background:black;color:white;"><body><p>Login successful. This window should close automatically. If it does not, you can close it manually.</p></body></html>"#;
const LOGIN_FAILURE_BODY: &str = r#"<html style="background:black;color:white;"><body><p>Authentication failed. You can close this window.</p></body></html>"#;

type OidcClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Clone, Debug)]
struct NativeAuthRuntimeConfig {
    client_id: String,
    token_endpoint: String,
    redirect_uri: String,
    scopes: Vec<String>,
    client: OidcClient,
}

impl NativeAuthRuntimeConfig {
    fn from_auth_config(config: &StdbAuthConfig) -> Result<Self, NativeAuthError> {
        let options = &config.options;
        let client = BasicClient::new(ClientId::new(options.client_id.clone()))
            .set_auth_uri(
                AuthUrl::new(options.auth_endpoint.clone())
                    .map_err(NativeAuthError::InvalidAuthUrl)?,
            )
            .set_token_uri(
                TokenUrl::new(options.token_endpoint.clone())
                    .map_err(NativeAuthError::InvalidTokenUrl)?,
            )
            .set_redirect_uri(
                RedirectUrl::new(options.redirect_uri.clone())
                    .map_err(NativeAuthError::InvalidRedirectUrl)?,
            );

        Ok(Self {
            client_id: options.client_id.clone(),
            token_endpoint: options.token_endpoint.clone(),
            redirect_uri: options.redirect_uri.clone(),
            scopes: options.scopes.clone(),
            client,
        })
    }
}

#[derive(Clone, Debug)]
struct LoopbackRedirect {
    host: String,
    port: u16,
    path: String,
}

impl LoopbackRedirect {
    fn parse(uri: &str) -> Result<Self, NativeAuthError> {
        let parsed = Url::parse(uri).map_err(NativeAuthError::Url)?;

        if parsed.scheme() != "http" {
            return Err(NativeAuthError::UnsupportedRedirectScheme(
                parsed.scheme().to_string(),
            ));
        }

        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(NativeAuthError::InvalidRedirectUri(
                "redirect URI must not include a query string or fragment".to_string(),
            ));
        }

        let host = parsed
            .host_str()
            .ok_or(NativeAuthError::MissingRedirectHost)?
            .to_string();
        let port = parsed.port().ok_or(NativeAuthError::MissingRedirectPort)?;
        let path = parsed.path().to_string();

        Ok(Self { host, port, path })
    }
}

#[derive(Clone, Debug)]
struct AuthorizationCallback {
    code: String,
    state: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRefreshToken {
    refresh_token: String,
}

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Debug)]
pub(crate) enum NativeAuthError {
    Io(std::io::Error),
    Url(url::ParseError),
    InvalidAuthUrl(url::ParseError),
    InvalidTokenUrl(url::ParseError),
    InvalidRedirectUrl(url::ParseError),
    InvalidRedirectUri(String),
    UnsupportedRedirectScheme(String),
    MissingRedirectHost,
    MissingRedirectPort,
    BrowserOpen(String),
    InvalidHttpRequest(String),
    CallbackPathMismatch {
        expected: String,
        actual: String,
    },
    MissingQueryParam(&'static str),
    OAuthError {
        error: String,
        error_description: Option<String>,
    },
    StateMismatch,
    Http(reqwest::Error),
    Json(serde_json::Error),
    TokenExchange(String),
}

impl Display for NativeAuthError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Url(error) => write!(f, "URL parse error: {error}"),
            Self::InvalidAuthUrl(error) => write!(f, "Invalid auth URL: {error}"),
            Self::InvalidTokenUrl(error) => write!(f, "Invalid token URL: {error}"),
            Self::InvalidRedirectUrl(error) => write!(f, "Invalid redirect URL: {error}"),
            Self::InvalidRedirectUri(message) => write!(f, "Invalid redirect URI: {message}"),
            Self::UnsupportedRedirectScheme(scheme) => {
                write!(f, "Unsupported redirect URI scheme: {scheme}")
            }
            Self::MissingRedirectHost => write!(f, "Redirect URI is missing a host"),
            Self::MissingRedirectPort => write!(f, "Redirect URI is missing an explicit port"),
            Self::BrowserOpen(error) => write!(f, "Failed to open browser: {error}"),
            Self::InvalidHttpRequest(message) => write!(f, "Invalid callback request: {message}"),
            Self::CallbackPathMismatch { expected, actual } => {
                write!(
                    f,
                    "Unexpected callback path: expected `{expected}`, received `{actual}`"
                )
            }
            Self::MissingQueryParam(param) => {
                write!(f, "Missing callback query parameter: {param}")
            }
            Self::OAuthError {
                error,
                error_description,
            } => match error_description {
                Some(description) => write!(f, "OAuth error: {error} ({description})"),
                None => write!(f, "OAuth error: {error}"),
            },
            Self::StateMismatch => write!(f, "OIDC state mismatch"),
            Self::Http(error) => write!(f, "HTTP error: {error}"),
            Self::Json(error) => write!(f, "JSON decode error: {error}"),
            Self::TokenExchange(message) => write!(f, "Token exchange failed: {message}"),
        }
    }
}

impl Error for NativeAuthError {}

impl From<std::io::Error> for NativeAuthError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<url::ParseError> for NativeAuthError {
    fn from(value: url::ParseError) -> Self {
        Self::Url(value)
    }
}

impl From<reqwest::Error> for NativeAuthError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<serde_json::Error> for NativeAuthError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub(crate) fn resolve_auth(
    options: &StdbAuthOptions,
    mode: PendingAuthMode,
    current_tokens: &CurrentAuthTokens,
) -> Result<TokenResponse, String> {
    let auth_config = StdbAuthConfig {
        options: options.clone(),
    };

    match mode {
        PendingAuthMode::Interactive => {
            println!("bevy_stdb native auth: starting interactive auth flow");
            authenticate(&auth_config).map_err(|error| error.to_string())
        }
        PendingAuthMode::Silent | PendingAuthMode::Reconnect => {
            let in_memory_refresh_token = current_tokens.refresh_token().map(ToOwned::to_owned);
            println!(
                "bevy_stdb native auth: attempting silent auth resolution, mode={:?}, in_memory_refresh_token={}",
                mode,
                in_memory_refresh_token.is_some()
            );

            let stored_refresh_token = if in_memory_refresh_token.is_none() {
                match load_refresh_token(&auth_config) {
                    Ok(token) => {
                        println!(
                            "bevy_stdb native auth: loaded refresh token from file storage, found={}",
                            token.is_some()
                        );
                        token
                    }
                    Err(error) => {
                        println!(
                            "bevy_stdb native auth: failed to load refresh token from file storage: {}",
                            error
                        );
                        None
                    }
                }
            } else {
                None
            };

            let refresh_token = in_memory_refresh_token.or(stored_refresh_token);

            let Some(refresh_token) = refresh_token else {
                println!(
                    "bevy_stdb native auth: no refresh token available for silent authentication"
                );
                return Err("No refresh token available for silent authentication.".to_string());
            };

            println!("bevy_stdb native auth: refreshing access token with refresh token");
            refresh(&auth_config, &refresh_token).map_err(|error| error.to_string())
        }
    }
}

pub(crate) fn store_tokens(
    options: &StdbAuthOptions,
    tokens: &TokenResponse,
) -> Result<(), String> {
    let auth_config = StdbAuthConfig {
        options: options.clone(),
    };

    persist_refresh_token(&auth_config, tokens.refresh_token.as_deref())
        .map_err(|error| error.to_string())
}

pub(crate) fn clear_stored_tokens(options: &StdbAuthOptions) -> Result<(), String> {
    let auth_config = StdbAuthConfig {
        options: options.clone(),
    };

    persist_refresh_token(&auth_config, None).map_err(|error| error.to_string())
}

pub(crate) fn authenticate(auth_config: &StdbAuthConfig) -> Result<TokenResponse, NativeAuthError> {
    let config = NativeAuthRuntimeConfig::from_auth_config(auth_config)?;
    let redirect = LoopbackRedirect::parse(&config.redirect_uri)?;

    let listener = TcpListener::bind((redirect.host.as_str(), redirect.port))?;

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let mut authorization_request = config.client.authorize_url(CsrfToken::new_random);

    for scope in &config.scopes {
        authorization_request = authorization_request.add_scope(Scope::new(scope.clone()));
    }

    let (auth_url, csrf_token) = authorization_request
        .set_pkce_challenge(pkce_challenge)
        .url();

    webbrowser::open(auth_url.as_str())
        .map_err(|error| NativeAuthError::BrowserOpen(error.to_string()))?;

    let callback = receive_oauth_callback(&listener, &redirect.path)?;
    if callback.state != csrf_token.secret().as_str() {
        return Err(NativeAuthError::StateMismatch);
    }

    exchange_authorization_code(&config, &callback.code, pkce_verifier)
}

pub(crate) fn refresh(
    auth_config: &StdbAuthConfig,
    refresh_token: &str,
) -> Result<TokenResponse, NativeAuthError> {
    let config = NativeAuthRuntimeConfig::from_auth_config(auth_config)?;
    let mut tokens = exchange_refresh_token(&config, refresh_token)?;

    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(refresh_token.to_string());
    }

    Ok(tokens)
}

pub(crate) fn load_refresh_token(
    auth_config: &StdbAuthConfig,
) -> Result<Option<String>, NativeAuthError> {
    match auth_config.options.storage {
        StdbTokenStorage::None => {
            println!("bevy_stdb native auth: token storage disabled, skipping refresh token load");
            Ok(None)
        }
        StdbTokenStorage::PlatformDefault => {
            let path = storage_file_path(auth_config)?;
            println!(
                "bevy_stdb native auth: reading refresh token file: {}",
                path.display()
            );

            match fs::read_to_string(&path) {
                Ok(value) => {
                    let stored: StoredRefreshToken = serde_json::from_str(&value)?;
                    if stored.refresh_token.trim().is_empty() {
                        println!("bevy_stdb native auth: refresh token file entry was blank");
                        Ok(None)
                    } else {
                        println!(
                            "bevy_stdb native auth: loaded refresh token from file storage; path={}",
                            path.display()
                        );
                        Ok(Some(stored.refresh_token))
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    println!(
                        "bevy_stdb native auth: no refresh token file found; path={}",
                        path.display()
                    );
                    Ok(None)
                }
                Err(error) => Err(NativeAuthError::Io(error)),
            }
        }
    }
}

pub(crate) fn persist_refresh_token(
    auth_config: &StdbAuthConfig,
    refresh_token: Option<&str>,
) -> Result<(), NativeAuthError> {
    match auth_config.options.storage {
        StdbTokenStorage::None => {
            println!(
                "bevy_stdb native auth: token storage disabled, skipping refresh token persistence"
            );
            Ok(())
        }
        StdbTokenStorage::PlatformDefault => {
            let path = storage_file_path(auth_config)?;

            match refresh_token {
                Some(refresh_token) if !refresh_token.trim().is_empty() => {
                    println!(
                        "bevy_stdb native auth: persisting refresh token to file storage; path={}",
                        path.display()
                    );

                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }

                    let payload = serde_json::to_string(&StoredRefreshToken {
                        refresh_token: refresh_token.to_string(),
                    })?;
                    fs::write(&path, payload.as_bytes())?;

                    match fs::read_to_string(&path) {
                        Ok(readback) => {
                            println!(
                                "bevy_stdb native auth: file readback after persist succeeded; path={}, payload_len={}",
                                path.display(),
                                readback.len()
                            );
                        }
                        Err(error) => {
                            println!(
                                "bevy_stdb native auth: file readback after persist failed; path={}, error={}",
                                path.display(),
                                error
                            );
                        }
                    }

                    Ok(())
                }
                _ => {
                    println!(
                        "bevy_stdb native auth: clearing refresh token from file storage; path={}",
                        path.display()
                    );
                    match fs::remove_file(&path) {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(error) => Err(NativeAuthError::Io(error)),
                    }
                }
            }
        }
    }
}

fn exchange_authorization_code(
    config: &NativeAuthRuntimeConfig,
    code: &str,
    code_verifier: PkceCodeVerifier,
) -> Result<TokenResponse, NativeAuthError> {
    let http_client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let oauth_token_response = config
        .client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(code_verifier)
        .request(&move |request: oauth2::HttpRequest| {
            http_client
                .execute(request.try_into().map_err(NativeAuthError::Http)?)
                .map_err(NativeAuthError::Http)
                .and_then(|response| {
                    let status_code = response.status();
                    let headers = response.headers().clone();
                    let body = response.bytes().map_err(NativeAuthError::Http)?.to_vec();

                    oauth2::http::Response::builder()
                        .status(status_code)
                        .body(body)
                        .map(|mut oauth_response| {
                            *oauth_response.headers_mut() = headers;
                            oauth_response
                        })
                        .map_err(|error| NativeAuthError::TokenExchange(error.to_string()))
                })
        })
        .map_err(|error| NativeAuthError::TokenExchange(error.to_string()))?;

    let response_json = serde_json::to_value(&oauth_token_response)?;
    let raw = RawTokenResponse {
        access_token: OAuthTokenResponse::access_token(&oauth_token_response)
            .secret()
            .to_string(),
        expires_in: response_json
            .get("expires_in")
            .and_then(|value| value.as_u64()),
        refresh_token: response_json
            .get("refresh_token")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        scope: response_json
            .get("scope")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        token_type: response_json
            .get("token_type")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        id_token: response_json
            .get("id_token")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
    };

    Ok(raw.into())
}

fn exchange_refresh_token(
    config: &NativeAuthRuntimeConfig,
    refresh_token: &str,
) -> Result<TokenResponse, NativeAuthError> {
    println!(
        "bevy_stdb native auth: sending refresh token request, token_endpoint={}, client_id={}, refresh_token_len={}",
        config.token_endpoint,
        config.client_id,
        refresh_token.len()
    );

    let http_client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let response = http_client
        .post(&config.token_endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", config.client_id.as_str()),
        ])
        .send()?;

    let status = response.status();
    println!(
        "bevy_stdb native auth: received refresh token response, status={}",
        status
    );

    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        println!(
            "bevy_stdb native auth: refresh token request failed, status={}, body={}",
            status, body
        );
        return Err(NativeAuthError::TokenExchange(body));
    }

    let raw: RawTokenResponse = response.json()?;
    println!(
        "bevy_stdb native auth: refresh token exchange succeeded, has_refresh_token={}, expires_in={}",
        raw.refresh_token.is_some(),
        raw.expires_in.unwrap_or_default()
    );
    Ok(raw.into())
}

fn receive_oauth_callback(
    listener: &TcpListener,
    expected_path: &str,
) -> Result<AuthorizationCallback, NativeAuthError> {
    let (mut stream, _) = listener.accept()?;
    let request_target = read_request_target(&mut stream)?;
    let callback_url = parse_request_target(&request_target)?;

    if callback_url.path() != expected_path {
        write_html_response(&mut stream, 400, LOGIN_FAILURE_BODY)?;
        return Err(NativeAuthError::CallbackPathMismatch {
            expected: expected_path.to_string(),
            actual: callback_url.path().to_string(),
        });
    }

    let error = callback_url
        .query_pairs()
        .find(|(key, _)| key == "error")
        .map(|(_, value)| value.to_string());

    if let Some(error) = error {
        let error_description = callback_url
            .query_pairs()
            .find(|(key, _)| key == "error_description")
            .map(|(_, value)| value.to_string());

        write_html_response(&mut stream, 400, LOGIN_FAILURE_BODY)?;

        return Err(NativeAuthError::OAuthError {
            error,
            error_description,
        });
    }

    let code = callback_url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| {
            let _ = write_html_response(&mut stream, 400, LOGIN_FAILURE_BODY);
            NativeAuthError::MissingQueryParam("code")
        })?;

    let state = callback_url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| {
            let _ = write_html_response(&mut stream, 400, LOGIN_FAILURE_BODY);
            NativeAuthError::MissingQueryParam("state")
        })?;

    write_html_response(&mut stream, 200, LOGIN_SUCCESS_BODY)?;

    Ok(AuthorizationCallback { code, state })
}

fn parse_request_target(request_target: &str) -> Result<Url, NativeAuthError> {
    if request_target.starts_with("http://") || request_target.starts_with("https://") {
        Url::parse(request_target).map_err(NativeAuthError::Url)
    } else {
        Url::parse(&format!("http://localhost{request_target}")).map_err(NativeAuthError::Url)
    }
}

fn read_request_target(stream: &mut TcpStream) -> Result<String, NativeAuthError> {
    let mut buffer = [0_u8; 8192];
    let bytes_read = stream.read(&mut buffer)?;

    if bytes_read == 0 {
        return Err(NativeAuthError::InvalidHttpRequest(
            "empty request".to_string(),
        ));
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let request_line = request
        .lines()
        .next()
        .ok_or_else(|| NativeAuthError::InvalidHttpRequest("missing request line".to_string()))?;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();

    if method != "GET" || target.is_empty() {
        return Err(NativeAuthError::InvalidHttpRequest(
            request_line.to_string(),
        ));
    }

    Ok(target.to_string())
}

fn write_html_response(
    stream: &mut TcpStream,
    status_code: u16,
    body: &str,
) -> Result<(), NativeAuthError> {
    let status_text = match status_code {
        200 => "OK",
        400 => "Bad Request",
        _ => "OK",
    };

    let auto_close_script = if status_code == 200 {
        "<script>setTimeout(function(){window.open('', '_self');window.close();},5000);</script>"
    } else {
        ""
    };

    let html = format!("<!doctype html><html><body><p>{body}</p>{auto_close_script}</body></html>");
    let response = format!(
        "HTTP/1.1 {status_code} {status_text}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );

    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn storage_file_path(auth_config: &StdbAuthConfig) -> Result<PathBuf, NativeAuthError> {
    let mut path = dirs::config_dir().ok_or_else(|| {
        NativeAuthError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "platform config directory is unavailable",
        ))
    })?;
    path.push(STORAGE_DIR_NAME);
    path.push(storage_file_name(auth_config));
    Ok(path)
}

fn storage_file_name(auth_config: &StdbAuthConfig) -> String {
    format!(
        "{}_{}.json",
        STORAGE_FILE_PREFIX,
        sanitize_storage_component(&auth_config.options.client_id)
    )
}

fn sanitize_storage_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }

    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

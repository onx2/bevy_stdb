use super::{
    StdbAuthError, StdbOidcAuthOptions, StdbTokenResponse,
    common::{AUTH_ENDPOINT, OidcTokenResponse, TOKEN_ENDPOINT, authorization_redirect},
};
use crate::log::{error, info};
use oauth2::{
    AuthUrl, AuthorizationCode, Client, ClientId, CsrfToken, HttpClientError, HttpRequest,
    PkceCodeChallenge, RedirectUrl, Scope, StandardRevocableToken, TokenResponse as _, TokenUrl,
    basic::{BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse},
};

type OidcClient = Client<
    BasicErrorResponse,
    OidcTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
>;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    time::Duration,
};
use url::Url;

/// Acquires a token response using the native OIDC authorization code flow.
pub(crate) fn acquire_token_response(
    options: &StdbOidcAuthOptions,
) -> Result<StdbTokenResponse, StdbAuthError> {
    info!(
        "starting native OIDC authentication with client_id={} and redirect_uri={}",
        options.client_id, options.redirect_uri
    );

    let redirect_uri = Url::parse(&options.redirect_uri).map_err(|error| {
        error!("invalid OIDC redirect URI: {error}");
        StdbAuthError::Internal(format!("invalid OIDC redirect URI: {error}"))
    })?;

    let listener = bind_redirect_listener(&redirect_uri)?;

    let oauth_client = OidcClient::new(ClientId::new(options.client_id.clone()))
        .set_auth_uri(AuthUrl::new(AUTH_ENDPOINT.to_string()).map_err(|error| {
            error!("invalid OIDC authorization endpoint: {error}");
            StdbAuthError::Internal(format!("invalid OIDC authorization endpoint: {error}"))
        })?)
        .set_token_uri(TokenUrl::new(TOKEN_ENDPOINT.to_string()).map_err(|error| {
            error!("invalid OIDC token endpoint: {error}");
            StdbAuthError::Internal(format!("invalid OIDC token endpoint: {error}"))
        })?)
        .set_redirect_uri(
            RedirectUrl::new(options.redirect_uri.clone()).map_err(|error| {
                error!("invalid OIDC redirect URL: {error}");
                StdbAuthError::Internal(format!("invalid OIDC redirect URL: {error}"))
            })?,
        );

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let mut authorize_request = oauth_client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(pkce_challenge);

    if let Some(prompt) = options.prompt.as_param() {
        authorize_request = authorize_request.add_extra_param("prompt", prompt);
    }

    for scope in &options.scopes {
        authorize_request = authorize_request.add_scope(Scope::new(scope.clone()));
    }

    let (auth_url, csrf_token) = authorize_request.url();

    info!("opening OIDC authorization URL in browser");

    webbrowser::open(auth_url.as_str()).map_err(|error| {
        error!("failed to open OIDC authorization URL: {error}");
        StdbAuthError::Internal(format!("failed to open OIDC authorization URL: {error}"))
    })?;

    let redirect_url = wait_for_redirect(listener, &redirect_uri)?;

    let redirect = authorization_redirect(&redirect_url).ok_or_else(|| {
        error!("OIDC redirect did not include an authorization code and state");
        StdbAuthError::Internal(
            "OIDC redirect did not include an authorization code and state".to_string(),
        )
    })?;

    if redirect.state != *csrf_token.secret() {
        error!("OIDC redirect state did not match the original CSRF token");
        return Err(StdbAuthError::Internal(
            "OIDC redirect state did not match the original CSRF token".to_string(),
        ));
    }

    info!("OIDC redirect received; exchanging authorization code for token");

    let http_client = reqwest::blocking::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| {
            error!("failed to create OIDC token exchange HTTP client: {error}");
            StdbAuthError::Internal(format!(
                "failed to create OIDC token exchange HTTP client: {error}"
            ))
        })?;

    let token = oauth_client
        .exchange_code(AuthorizationCode::new(redirect.code))
        .set_pkce_verifier(pkce_verifier)
        .request(&|request: HttpRequest| {
            let mut response = http_client
                .execute(request.try_into().map_err(Box::new)?)
                .map_err(Box::new)?;

            let mut builder = oauth2::http::Response::builder()
                .status(response.status())
                .version(response.version());

            for (name, value) in response.headers() {
                builder = builder.header(name, value);
            }

            let mut body = Vec::new();
            response.read_to_end(&mut body)?;

            builder.body(body).map_err(HttpClientError::Http)
        })
        .map_err(|error| {
            error!("OIDC authorization code exchange failed: {error}");
            StdbAuthError::Internal(format!("OIDC authorization code exchange failed: {error}"))
        })?;

    info!(
        "OIDC authentication succeeded; received access token with expires_in={:?}, refresh_token_present={}",
        token.expires_in(),
        token.refresh_token().is_some()
    );

    Ok((&token).into())
}

fn bind_redirect_listener(redirect_uri: &Url) -> Result<TcpListener, StdbAuthError> {
    let host = redirect_uri.host_str().ok_or_else(|| {
        error!("OIDC redirect URI is missing a host");
        StdbAuthError::Internal("OIDC redirect URI is missing a host".to_string())
    })?;

    let port = redirect_uri.port_or_known_default().ok_or_else(|| {
        error!("OIDC redirect URI is missing a port");
        StdbAuthError::Internal("OIDC redirect URI is missing a port".to_string())
    })?;

    let bind_addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&bind_addr).map_err(|error| {
        error!("failed to bind OIDC redirect listener at {bind_addr}: {error}");
        StdbAuthError::Internal(format!(
            "failed to bind OIDC redirect listener at {bind_addr}: {error}"
        ))
    })?;

    listener.set_nonblocking(false).map_err(|error| {
        error!("failed to configure OIDC redirect listener: {error}");
        StdbAuthError::Internal(format!(
            "failed to configure OIDC redirect listener: {error}"
        ))
    })?;

    info!("listening for OIDC redirect at {bind_addr}");

    Ok(listener)
}

fn wait_for_redirect(listener: TcpListener, redirect_uri: &Url) -> Result<Url, StdbAuthError> {
    listener.set_ttl(64).map_err(|error| {
        error!("failed to configure OIDC redirect listener TTL: {error}");
        StdbAuthError::Internal(format!(
            "failed to configure OIDC redirect listener TTL: {error}"
        ))
    })?;

    let (mut stream, _) = listener.accept().map_err(|error| {
        error!("failed to accept OIDC redirect request: {error}");
        StdbAuthError::Internal(format!("failed to accept OIDC redirect request: {error}"))
    })?;

    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|error| {
            error!("failed to set OIDC redirect read timeout: {error}");
            StdbAuthError::Internal(format!("failed to set OIDC redirect read timeout: {error}"))
        })?;

    let request_target = read_request_target(&mut stream)?;
    let redirect_url = resolve_redirect_target(redirect_uri, &request_target)?;

    write_redirect_response(&mut stream)?;

    Ok(redirect_url)
}

fn read_request_target(stream: &mut TcpStream) -> Result<String, StdbAuthError> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();

    reader.read_line(&mut request_line).map_err(|error| {
        error!("failed to read OIDC redirect request line: {error}");
        StdbAuthError::Internal(format!(
            "failed to read OIDC redirect request line: {error}"
        ))
    })?;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or_else(|| {
        error!("OIDC redirect request line was empty");
        StdbAuthError::Internal("OIDC redirect request line was empty".to_string())
    })?;
    let target = parts.next().ok_or_else(|| {
        error!("OIDC redirect request line did not include a target");
        StdbAuthError::Internal("OIDC redirect request line did not include a target".to_string())
    })?;

    if method != "GET" {
        error!("OIDC redirect request used unsupported method: {method}");
        return Err(StdbAuthError::Internal(format!(
            "OIDC redirect request used unsupported method: {method}"
        )));
    }

    Ok(target.to_string())
}

fn resolve_redirect_target(redirect_uri: &Url, request_target: &str) -> Result<Url, StdbAuthError> {
    if let Ok(url) = Url::parse(request_target) {
        return Ok(url);
    }

    redirect_uri.join(request_target).map_err(|error| {
        error!("invalid OIDC redirect request target `{request_target}`: {error}");
        StdbAuthError::Internal(format!(
            "invalid OIDC redirect request target `{request_target}`: {error}"
        ))
    })
}

fn write_redirect_response(stream: &mut TcpStream) -> Result<(), StdbAuthError> {
    let body = "Authentication complete. You can return to the application.";

    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .map_err(|error| {
        error!("failed to write OIDC redirect response: {error}");
        StdbAuthError::Internal(format!("failed to write OIDC redirect response: {error}"))
    })?;

    stream.flush().map_err(|error| {
        error!("failed to flush OIDC redirect response: {error}");
        StdbAuthError::Internal(format!("failed to flush OIDC redirect response: {error}"))
    })
}

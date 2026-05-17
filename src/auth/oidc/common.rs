use super::{StdbAuthError, StdbOidcAuthOptions, StdbTokenResponse};
use oauth2::{
    AuthUrl, Client, ClientId, CsrfToken, EndpointNotSet, EndpointSet, ExtraTokenFields,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, StandardRevocableToken,
    StandardTokenResponse, TokenResponse as _, TokenUrl,
    basic::{
        BasicErrorResponse, BasicRevocationErrorResponse, BasicTokenIntrospectionResponse,
        BasicTokenType,
    },
};
use url::Url;

use super::super::AUTH_URI_BASE;

/// Extra OIDC fields returned by the token endpoint.
#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct OidcExtraFields {
    /// The ID token, used as `id_token_hint` for session termination.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id_token: Option<String>,
}

impl ExtraTokenFields for OidcExtraFields {}

/// A SpacetimeDB OIDC token response including the [`OidcExtraFields`] `id_token` field.
pub(crate) type OidcTokenResponse = StandardTokenResponse<OidcExtraFields, BasicTokenType>;

/// OAuth client configured with authorization and token endpoints.
///
/// The endpoint generic parameters represent authorization, device authorization,
/// introspection, revocation, and token endpoint availability.
pub(crate) type OidcClient = Client<
    BasicErrorResponse,
    OidcTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

/// Prepared OIDC authorization request state.
pub(crate) struct OidcAuthorizationRequest {
    /// The authorization URL opened by native clients or assigned by browser clients.
    pub auth_url: Url,
    /// The CSRF token used to validate the authorization callback.
    pub csrf_token: CsrfToken,
    /// The PKCE verifier used during authorization code exchange.
    pub pkce_verifier: PkceCodeVerifier,
}

/// Creates an [`OidcClient`] for a SpacetimeDB OIDC client.
pub(crate) fn oauth_client(
    client_id: &str,
    redirect_uri: &str,
) -> Result<OidcClient, StdbAuthError> {
    Ok(Client::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(
            AuthUrl::new(format!("{AUTH_URI_BASE}/auth")).map_err(|error| {
                StdbAuthError::Internal(format!("invalid OIDC authorization endpoint: {error}"))
            })?,
        )
        .set_token_uri(
            TokenUrl::new(format!("{AUTH_URI_BASE}/token")).map_err(|error| {
                StdbAuthError::Internal(format!("invalid OIDC token endpoint: {error}"))
            })?,
        )
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string()).map_err(|error| {
            StdbAuthError::Internal(format!("invalid OIDC redirect URL: {error}"))
        })?))
}

/// Creates an authorization URL and callback validation state.
pub(crate) fn authorization_request(
    options: &StdbOidcAuthOptions,
) -> Result<OidcAuthorizationRequest, StdbAuthError> {
    let oauth_client = oauth_client(&options.client_id, &options.redirect_uri)?;
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

    Ok(OidcAuthorizationRequest {
        auth_url,
        csrf_token,
        pkce_verifier,
    })
}

/// Authorization callback parameters returned by the OIDC provider.
#[cfg_attr(feature = "browser", allow(dead_code))]
pub(crate) struct AuthorizationRedirect {
    /// The authorization code exchanged for a token response.
    pub code: String,
    /// The callback state used to validate the authorization request.
    pub state: String,
}

/// Extracts authorization callback parameters from a [`Url`].
#[cfg_attr(feature = "browser", allow(dead_code))]
pub(crate) fn authorization_redirect(url: &Url) -> Option<AuthorizationRedirect> {
    Some(AuthorizationRedirect {
        code: query_param(url, "code")?,
        state: query_param(url, "state")?,
    })
}

/// Returns a decoded query parameter from a [`Url`].
#[cfg_attr(feature = "browser", allow(dead_code))]
pub(crate) fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find_map(|(name, value)| (name == key).then(|| value.into_owned()))
}

/// Formats granted OAuth scopes as a scope parameter string.
pub(crate) fn scopes_param(scopes: &[Scope]) -> Option<String> {
    (!scopes.is_empty()).then(|| {
        scopes
            .iter()
            .map(|scope| scope.as_ref())
            .collect::<Vec<_>>()
            .join(" ")
    })
}

impl From<&OidcTokenResponse> for StdbTokenResponse {
    fn from(token: &OidcTokenResponse) -> Self {
        Self {
            access_token: token.access_token().secret().to_string(),
            token_type: format!("{:?}", token.token_type()),
            expires_in: token.expires_in().map(|duration| duration.as_secs()),
            refresh_token: token.refresh_token().map(|t| t.secret().to_string()),
            scope: token.scopes().and_then(|scopes| scopes_param(scopes)),
            id_token: token.extra_fields().id_token.clone(),
            post_logout_redirect_uri: None,
        }
    }
}

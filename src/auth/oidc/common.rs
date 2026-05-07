use super::StdbTokenResponse;
use oauth2::{Scope, TokenResponse as _};
use url::Url;

pub(crate) const AUTH_ENDPOINT: &str = "https://auth.spacetimedb.com/oidc/auth";
pub(crate) const TOKEN_ENDPOINT: &str = "https://auth.spacetimedb.com/oidc/token";
pub(crate) const END_SESSION_ENDPOINT: &str = "https://auth.spacetimedb.com/oidc/session/end";

/// Extra OIDC fields returned by the token endpoint.
#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct OidcExtraFields {
    /// The ID token, used as `id_token_hint` for session termination.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id_token: Option<String>,
}

impl oauth2::ExtraTokenFields for OidcExtraFields {}

/// A SpacetimeDB OIDC token response including the [`OidcExtraFields`] `id_token` field.
pub(crate) type OidcTokenResponse =
    oauth2::StandardTokenResponse<OidcExtraFields, oauth2::basic::BasicTokenType>;

pub(crate) struct AuthorizationRedirect {
    pub code: String,
    pub state: String,
}

pub(crate) fn authorization_redirect(url: &Url) -> Option<AuthorizationRedirect> {
    Some(AuthorizationRedirect {
        code: query_param(url, "code")?,
        state: query_param(url, "state")?,
    })
}

pub(crate) fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find_map(|(name, value)| (name == key).then(|| value.into_owned()))
}

pub(crate) fn scopes_param(scopes: &[Scope]) -> Option<String> {
    (!scopes.is_empty()).then(|| {
        scopes
            .iter()
            .map(|scope| scope.as_ref())
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// Converts an [`OidcTokenResponse`] into a [`StdbTokenResponse`].
pub(crate) fn token_response_from_oauth(token: &OidcTokenResponse) -> StdbTokenResponse {
    StdbTokenResponse {
        access_token: token.access_token().secret().to_string(),
        token_type: format!("{:?}", token.token_type()),
        expires_in: token.expires_in().map(|duration| duration.as_secs()),
        refresh_token: token
            .refresh_token()
            .map(|refresh_token| refresh_token.secret().to_string()),
        scope: token.scopes().and_then(|scopes| scopes_param(scopes)),
        id_token: token.extra_fields().id_token.clone(),
    }
}

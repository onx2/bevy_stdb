use super::StdbTokenResponse;
use oauth2::{
    ExtraTokenFields, Scope, StandardTokenResponse, TokenResponse as _, basic::BasicTokenType,
};
use url::Url;

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

impl From<&OidcTokenResponse> for StdbTokenResponse {
    fn from(token: &OidcTokenResponse) -> Self {
        Self {
            access_token: token.access_token().secret().to_string(),
            token_type: format!("{:?}", token.token_type()),
            expires_in: token.expires_in().map(|duration| duration.as_secs()),
            refresh_token: token.refresh_token().map(|t| t.secret().to_string()),
            scope: token.scopes().and_then(|scopes| scopes_param(scopes)),
            id_token: token.extra_fields().id_token.clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum StdbAuthError {
    Http(reqwest::Error),
    Decode(serde_json::Error),
    Steam(steamworks::SteamError),
    Timeout,
    Internal(String),
}

impl From<reqwest::Error> for StdbAuthError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<serde_json::Error> for StdbAuthError {
    fn from(value: serde_json::Error) -> Self {
        Self::Decode(value)
    }
}

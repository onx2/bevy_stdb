#[derive(Debug)]
pub(crate) enum StdbAuthError {
    Http(ureq::Error),
    Decode(serde_json::Error),
    Steam(steamworks::SteamError),
    Timeout,
    Internal(String),
}

impl From<ureq::Error> for StdbAuthError {
    fn from(value: ureq::Error) -> Self {
        Self::Http(value)
    }
}

impl From<serde_json::Error> for StdbAuthError {
    fn from(value: serde_json::Error) -> Self {
        Self::Decode(value)
    }
}

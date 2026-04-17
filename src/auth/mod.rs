/// Authentication options for a SpacetimeDB connection.
#[derive(Clone, Debug)]
pub struct StdbAuthOptions {
    pub client_id: String,
    pub auth_endpoint: String,
    pub token_endpoint: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub startup_behavior: StdbAuthStartupBehavior,
}

#[derive(Clone, Debug, Default)]
pub enum StdbAuthStartupBehavior {
    /// First attempts to use stored refresh token, then uses interaction mode
    #[default]
    SilentFirst,
    /// Skips refresh token attempt and always require user interaction
    Interactive,
}

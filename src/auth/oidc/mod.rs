#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod auth_imp;

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod auth_imp;

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::Resource;

#[derive(Clone, Debug)]
pub struct StdbOidcAuthOptions {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The redirect URI used by the client.
    pub redirect_uri: String,
    /// The requested scopes.
    pub scopes: Vec<String>,
}

/// Stores the configured auth options.
#[derive(Resource, Clone, Debug)]
pub(crate) struct StdbOidcAuthConfig(pub StdbOidcAuthOptions);

pub struct StdbOidcAuthPlugin {
    options: StdbOidcAuthOptions,
}
impl StdbOidcAuthPlugin {
    pub fn new(options: StdbOidcAuthOptions) -> Self {
        Self { options }
    }
}
impl Plugin for StdbOidcAuthPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(StdbOidcAuthConfig(self.options.clone()));
        // TODO
    }
}

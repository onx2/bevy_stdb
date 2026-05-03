use crate::{
    auth::StdbAuthSource,
    message::{
        StdbConnectOptions, StdbConnectRequest, StdbDisconnectOptions, StdbDisconnectRequest,
        StdbLoginOptions, StdbLoginRequest, StdbLogoutOptions, StdbLogoutRequest,
    },
};
use bevy_ecs::{prelude::MessageWriter, system::SystemParam};

/// Sends SpacetimeDB commands from Bevy systems.
#[derive(SystemParam)]
pub struct StdbCommands<'w> {
    login_requests: MessageWriter<'w, StdbLoginRequest>,
    logout_requests: MessageWriter<'w, StdbLogoutRequest>,
    connect_requests: MessageWriter<'w, StdbConnectRequest>,
    disconnect_requests: MessageWriter<'w, StdbDisconnectRequest>,
}

impl StdbCommands<'_> {
    /// Requests authentication with a [`StdbAuthSource`].
    pub fn login(&mut self, auth_source: StdbAuthSource) {
        self.login_with(StdbLoginOptions::new(auth_source));
    }

    /// Requests authentication with [`StdbLoginOptions`].
    pub fn login_with(&mut self, options: StdbLoginOptions) {
        self.login_requests.write(StdbLoginRequest { options });
    }

    /// Requests stored authentication to be cleared.
    pub fn logout(&mut self) {
        self.logout_with(StdbLogoutOptions::default());
    }

    /// Requests stored authentication to be cleared with [`StdbLogoutOptions`].
    pub fn logout_with(&mut self, options: StdbLogoutOptions) {
        self.logout_requests.write(StdbLogoutRequest { options });
    }

    /// Requests a SpacetimeDB connection attempt.
    pub fn connect(&mut self) {
        self.connect_with(StdbConnectOptions::default());
    }

    /// Requests a SpacetimeDB connection attempt with [`StdbConnectOptions`].
    pub fn connect_with(&mut self, options: StdbConnectOptions) {
        self.connect_requests.write(StdbConnectRequest { options });
    }

    /// Requests disconnection from SpacetimeDB.
    pub fn disconnect(&mut self) {
        self.disconnect_with(StdbDisconnectOptions);
    }

    /// Requests disconnection from SpacetimeDB with [`StdbDisconnectOptions`].
    pub fn disconnect_with(&mut self, options: StdbDisconnectOptions) {
        self.disconnect_requests
            .write(StdbDisconnectRequest { options });
    }
}

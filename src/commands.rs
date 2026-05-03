use crate::message::{
    StdbConnectOptions, StdbConnectRequest, StdbDisconnectOptions, StdbDisconnectRequest,
    StdbLoginOptions, StdbLoginRequest, StdbLogoutOptions, StdbLogoutRequest,
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
    /// Requests authentication using [`StdbLoginOptions`].
    pub fn login(&mut self, options: StdbLoginOptions) {
        self.login_requests.write(StdbLoginRequest { options });
    }

    /// Requests stored authentication to be cleared using [`StdbLogoutOptions`].
    pub fn logout(&mut self, options: StdbLogoutOptions) {
        self.logout_requests.write(StdbLogoutRequest { options });
    }

    /// Requests a SpacetimeDB connection attempt using [`StdbConnectOptions`].
    pub fn connect(&mut self, options: StdbConnectOptions) {
        self.connect_requests.write(StdbConnectRequest { options });
    }

    /// Requests disconnection from SpacetimeDB using [`StdbDisconnectOptions`].
    pub fn disconnect(&mut self, options: StdbDisconnectOptions) {
        self.disconnect_requests
            .write(StdbDisconnectRequest { options });
    }
}

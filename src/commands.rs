use crate::message::{StdbConnectRequest, StdbDisconnectRequest};
use bevy_ecs::{prelude::MessageWriter, system::SystemParam};

/// Sends SpacetimeDB connection requests from Bevy systems.
#[derive(SystemParam)]
pub struct StdbCommands<'w> {
    connect_requests: MessageWriter<'w, StdbConnectRequest>,
    disconnect_requests: MessageWriter<'w, StdbDisconnectRequest>,
}

impl StdbCommands<'_> {
    /// Requests a SpacetimeDB connection attempt.
    pub fn connect(&mut self, request: StdbConnectRequest) {
        self.connect_requests.write(request);
    }

    /// Requests disconnection from SpacetimeDB.
    pub fn disconnect(&mut self, request: StdbDisconnectRequest) {
        self.disconnect_requests.write(request);
    }
}

use bevy_app::{App, Plugin};

#[derive(Debug, Default)]
pub struct StdbAuthPlugin;
impl Plugin for StdbAuthPlugin {
    fn build(&self, app: &mut App) {
        // - auto-refresh of access_token
        // - persist refresh token on app close (bevy shutdown)
    }
}

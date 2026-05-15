use bevy_log::{error, info};
use keyring::Entry;

pub(crate) const KEYRING_SERVICE: &str = "bevy_stdb";

/// Returns the stored OIDC refresh token for the given `client_id`, if any.
pub(crate) fn stored_refresh_token(client_id: &str) -> Option<String> {
    let entry = match Entry::new(KEYRING_SERVICE, client_id) {
        Ok(entry) => entry,
        Err(error) => {
            error!(
                "failed to open keyring service `{KEYRING_SERVICE}` entry for OIDC refresh token client_id={client_id}: {error}"
            );
            return None;
        }
    };

    match entry.get_password() {
        Ok(refresh_token) => {
            info!(
                "loaded OIDC refresh token from keyring service `{KEYRING_SERVICE}` for client_id={client_id}"
            );
            Some(refresh_token)
        }
        Err(error) => {
            info!(
                "no OIDC refresh token available in keyring service `{KEYRING_SERVICE}` for client_id={client_id}: {error}"
            );
            None
        }
    }
}

/// Persists an OIDC refresh token for the given `client_id` to the system keyring.
pub(crate) fn store_refresh_token(client_id: &str, refresh_token: &str) {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, client_id) {
        Ok(entry) => entry,
        Err(error) => {
            error!(
                "failed to open keyring service `{KEYRING_SERVICE}` entry for storing OIDC refresh token client_id={client_id}: {error}"
            );
            return;
        }
    };

    match entry.set_password(refresh_token) {
        Ok(()) => {
            info!(
                "stored OIDC refresh token in keyring service `{KEYRING_SERVICE}` for client_id={client_id}"
            );
        }
        Err(error) => {
            error!(
                "failed to store OIDC refresh token in keyring for client_id={client_id}: {error}"
            );
        }
    }
}

/// Removes the stored OIDC refresh token for the given `client_id` from the system keyring.
pub(crate) fn clear_stored_refresh_token(client_id: &str) {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, client_id) {
        Ok(entry) => entry,
        Err(error) => {
            error!(
                "failed to open keyring service `{KEYRING_SERVICE}` entry for clearing OIDC refresh token client_id={client_id}: {error}"
            );
            return;
        }
    };

    match entry.delete_credential() {
        Ok(()) => {
            info!(
                "cleared OIDC refresh token from keyring service `{KEYRING_SERVICE}` for client_id={client_id}"
            );
        }
        Err(error) => {
            error!(
                "failed to clear OIDC refresh token from keyring for client_id={client_id}: {error}"
            );
        }
    }
}

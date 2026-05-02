use super::{StdbAuthError, StdbTokenResponse};
use crate::log::{error, info};
use std::{
    thread,
    time::{Duration, Instant},
};
use steamworks::{Client, TicketForWebApiResponse};

#[derive(Clone, Debug)]
pub struct StdbSteamAuthOptions {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The unique identifier for your Steam game.
    pub app_id: u32,
}

/// Acquires a token response using Steam authentication.
pub fn acquire_token_response(
    options: &StdbSteamAuthOptions,
) -> Result<StdbTokenResponse, StdbAuthError> {
    info!(
        "starting Steam authentication with app_id={} and client_id={}",
        options.app_id, options.client_id
    );

    let steam_client = Client::init_app(options.app_id).map_err(|error| {
        error!("failed to initialize Steam client: {error}");
        StdbAuthError::Internal(format!("failed to init Steam client: {error}"))
    })?;

    info!("Steam client initialized; requesting Web API ticket");

    let ticket = request_steam_webapi_ticket(&steam_client)?;

    info!("received Steam Web API ticket; exchanging for SpacetimeAuth token");

    let token = exchange_steam_ticket_request(&options.client_id, &ticket)?;

    info!(
        "Steam authentication succeeded; received access token with expires_in={:?}, refresh_token_present={}",
        token.expires_in,
        token.refresh_token.is_some()
    );

    Ok(token)
}

/// Exchanges a Steam Web API ticket for a token response.
fn exchange_steam_ticket_request(
    client_id: &str,
    steam_ticket: &[u8],
) -> Result<StdbTokenResponse, StdbAuthError> {
    info!(
        "exchanging Steam Web API ticket with SpacetimeAuth; ticket_bytes={}",
        steam_ticket.len()
    );

    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://auth.spacetimedb.com/oidc/token")
        .form(&[
            ("grant_type", "urn:spacetimeauth:steam-ticket"),
            ("steam_ticket", hex::encode(steam_ticket).as_str()),
            ("client_id", client_id),
        ])
        .send()
        .map_err(|error| {
            error!("failed to send Steam ticket exchange request: {error}");
            StdbAuthError::from(error)
        })?
        .error_for_status()
        .map_err(|error| {
            error!("Steam ticket exchange returned an error status: {error}");
            StdbAuthError::from(error)
        })?;

    let token_data = response.json::<StdbTokenResponse>().map_err(|error| {
        error!("failed to decode Steam token exchange response: {error}");
        StdbAuthError::from(error)
    })?;

    Ok(token_data)
}

/// Requests a Steam Web API ticket.
fn request_steam_webapi_ticket(client: &Client) -> Result<Vec<u8>, StdbAuthError> {
    let (tx, rx) = crossbeam_channel::bounded(1);

    let requested_handle = client
        .user()
        .authentication_session_ticket_for_webapi("spacetimeauth");

    info!("requested Steam Web API ticket; waiting for callback");

    let _cb = client.register_callback(move |response: TicketForWebApiResponse| {
        if response.ticket_handle == requested_handle {
            info!("received Steam Web API ticket callback");
            let _ = tx.send(response.result.map(|()| response.ticket));
        }
    });

    let timeout = Duration::from_secs(5);
    let start = Instant::now();

    loop {
        client.run_callbacks();

        if let Ok(result) = rx.try_recv() {
            return result.map_err(|err| {
                error!("Steam Web API ticket request failed: {err}");
                StdbAuthError::Internal(err.to_string())
            });
        }

        if start.elapsed() >= timeout {
            error!("Steam Web API ticket request timed out after {:?}", timeout);
            return Err(StdbAuthError::Timeout);
        }

        thread::sleep(Duration::from_millis(10));
    }
}

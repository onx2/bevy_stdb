use super::{StdbAuthError, StdbTokenResponse};
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
    let steam_client = Client::init_app(options.app_id).map_err(|error| {
        StdbAuthError::Internal(format!("failed to init Steam client: {error}"))
    })?;
    let ticket = request_steam_webapi_ticket(&steam_client)?;
    let token = exchange_steam_ticket_request(&options.client_id, &ticket)?;

    Ok(token)
}
/// Exchanges a Steam Web API ticket for a token response.
fn exchange_steam_ticket_request(
    client_id: &str,
    steam_ticket: &[u8],
) -> Result<StdbTokenResponse, StdbAuthError> {
    let response = ureq::post("https://auth.spacetimedb.com/oidc/token")
        .content_type("application/x-www-form-urlencoded")
        .send_form([
            ("grant_type", "urn:spacetimeauth:steam-ticket"),
            ("steam_ticket", hex::encode(steam_ticket).as_str()),
            ("client_id", client_id),
        ])?;

    let body = response.into_body().read_to_string()?;
    let token_data: StdbTokenResponse = serde_json::from_str(&body)?;

    Ok(token_data)
}

/// Requests a Steam Web API ticket
fn request_steam_webapi_ticket(client: &Client) -> Result<Vec<u8>, StdbAuthError> {
    let (tx, rx) = crossbeam_channel::bounded(1);

    let requested_handle = client
        .user()
        .authentication_session_ticket_for_webapi("spacetimeauth");

    let _cb = client.register_callback(move |response: TicketForWebApiResponse| {
        if response.ticket_handle == requested_handle {
            let _ = tx.send(response.result.map(|()| response.ticket));
        }
    });

    let timeout = Duration::from_secs(5); // TODO: maybe allow configuration of timeout?
    let start = Instant::now();

    loop {
        client.run_callbacks();

        if let Ok(result) = rx.try_recv() {
            return result.map_err(|err| StdbAuthError::Internal(err.to_string()));
        }

        if start.elapsed() >= timeout {
            return Err(StdbAuthError::Timeout);
        }

        thread::sleep(Duration::from_millis(10));
    }
}

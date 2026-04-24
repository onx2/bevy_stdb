// use std::{thread, time::Duration};

// use steamworks::{Client, TicketForWebApiResponse};

// fn main() {
//     let client = Client::init_app(2717550).expect("Failed to initialize Steam client");

//     let _cb = client.register_callback(move |rq: TicketForWebApiResponse| {
//         if rq.result.is_ok() {
//             spacetimeauth_authentication(&rq.ticket);
//         } else {
//             panic!(
//                 "Failed to get authentication session ticket: {:?}",
//                 rq.result
//             );
//         }
//     });

//     client
//         .user()
//         .authentication_session_ticket_for_webapi("spacetimeauth");

//     loop {
//         client.run_callbacks();
//         thread::sleep(Duration::from_millis(100));
//     }
// }

// fn spacetimeauth_authentication(ticket: &[u8]) {
//     let hex_ticket: String = hex::encode(ticket);

//     let spacetimeauth_url = std::env::var("SPACETIMEAUTH_URL").unwrap();
//     let client_id = std::env::var("SPACETIMEAUTH_CLIENT_ID").unwrap();
//     let token_url = format!("{}/oidc/token", spacetimeauth_url);

//     let client = reqwest::blocking::Client::new();
//     let body = [
//         ("grant_type", "urn:spacetimeauth:steam-ticket"),
//         ("client_id", &client_id),
//         ("steam_ticket", hex_ticket.as_str()),
//     ];
//     let response = client.post(&token_url).form(&body).send().unwrap();
//     let status = response.status();
//     let body = response.text().unwrap();

//     println!("[{}] - {}", status, body); // Tokens are available here

//     std::process::exit(0);
// }

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::Resource;
use std::{
    thread,
    time::{Duration, Instant},
};
use steamworks::{Client, SteamError, TicketForWebApiResponse};

#[derive(Clone, Debug)]
pub struct StdbSteamAuthOptions {
    /// The OAuth client identifier.
    pub client_id: String,
    /// The unique identifier for your Steam game.
    pub app_id: usize,
}

/// Stores the configured auth options.
#[derive(Resource, Clone, Debug)]
pub(crate) struct StdbSteamAuthConfig(pub StdbSteamAuthOptions);

pub struct StdbSteamAuthPlugin {
    options: StdbSteamAuthOptions,
}
impl StdbSteamAuthPlugin {
    pub fn new(options: StdbSteamAuthOptions) -> Self {
        Self { options }
    }
}
impl Plugin for StdbSteamAuthPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(StdbSteamAuthConfig(self.options.clone()));
        // TODO
    }
}

pub fn authenticate(options: &StdbSteamAuthOptions) -> Option<String> {
    None
}

fn exchange_steam_ticket_request(
    client_id: &str,
    steam_ticket: &[u8],
) -> Result<String, ureq::Error> {
    let steam_ticket = hex::encode(steam_ticket);
    let response = ureq::post("https://auth.spacetimedb.com/oidc/token")
        .content_type("application/x-www-form-urlencoded")
        .send_form([
            ("grant_type", "urn:spacetimeauth:steam-ticket"),
            ("steam_ticket", steam_ticket.as_str()),
            ("client_id", client_id),
        ])?;

    response.into_body().read_to_string()
}

/// Requests a Steam Web API ticket for `identity`.
///
/// Blocks the current thread while polling Steam callbacks until the matching
/// [`steamworks::TicketForWebApiResponse`] arrives or the request times out.
///
/// The returned ticket bytes can then be exchanged at the auth server token endpoint.
fn request_steam_webapi_ticket(client: &Client) -> Result<Vec<u8>, SteamError> {
    let (tx, rx) = crossbeam_channel::bounded(1);

    let requested_handle = client
        .user()
        .authentication_session_ticket_for_webapi("spacetimeauth");

    let _cb = client.register_callback(move |response: TicketForWebApiResponse| {
        if response.ticket_handle == requested_handle {
            let _ = tx.send(response.result.map(|()| response.ticket));
        }
    });

    let timeout = Duration::from_secs(5);
    let start = Instant::now();

    loop {
        client.run_callbacks();

        if let Ok(result) = rx.try_recv() {
            return result;
        }

        if start.elapsed() >= timeout {
            return Err(SteamError::Timeout);
        }

        thread::sleep(Duration::from_millis(10));
    }
}

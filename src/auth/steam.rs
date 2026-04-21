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

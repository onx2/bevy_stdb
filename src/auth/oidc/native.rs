/*
 * OIDC Flow
 * 1. RequestStdbConnectionMessage
 *    - auth_target: StdbAuthTarget::Oidc(OidcOptions { ... }) -> Pulled from config
 * 2. handle_connection_request (when `auth_target` exists)
 *     - Build OIDC client (PKDE, auth_url, etc...)
 *     - Check for a refresh token in peristed store
 *       - YES:
 *         - client.exchange_refresh_token(refresh_token).request(...) -> /token endpoint called
 *       - NO:
 *         - Start listening on the redirect_uri
 *         - Open the auth_url using webbrowser crate, user authenticates then:
 *         - On redirected to `redirect_uri`, listener can respond by:
 *           - parse response to get `code` and `state`
 *           - verify `state`
 *           - client.exchange_code(code).request(...) -> /token endpoint called
 *     - When the request finishes (Maybe a message to consolidate finalization logic between OIDC + Steam)
 *       - Parse response to get token
 *       - Verify token using `jsonwebtoken`
 *       - Store token details in a Resource (new one maybe, `StdbAuth`?)
 *       - Persist the refresh token in platform secure store
 *       - Connect to Spacetime using access token
 *
 *
 * Steam Flow
 * 1. RequestStdbConnectionMessage
 *    - auth_target: StdbAuthTarget::Steam(SteamOptions { ... }) -> Pulled from config
 * 2. handle_connection_request (when `auth_target` exists)
 *     - Build Steam client: steam_client = Client::init_app(steam_app_id) -> pulled from config
 *     - Request ticket: steam_client.users().authentication_session_ticket_for_webapi("spacetimeauth")
 *     - Listen for the AuthTicket response ticket [u8]
 *     - hex::encode the [u8] ticket received from listener
 *     - Send `ureq` http request to /token endpoint to get the spacetime token
 *     - When the request finishes (Maybe a message to consolidate finalization logic between OIDC + Steam)
 *       - Parse response to get token
 *       - Verify token using `jsonwebtoken`
 *       - Store token details in a Resource (new one maybe, `StdbAuth`?)
 *       - Persist the refresh token in platform secure store
 *       - Connect to Spacetime using access token
 */

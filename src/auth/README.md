# Authentication implementation game plan

## Goal

Add a first-class OIDC auth layer to `bevy_stdb` that:

- runs the standard authorization code + PKCE flow
- works on native and `wasm32`
- integrates into the existing connection lifecycle
- optionally persists a `refresh_token`
- supports refresh-first startup and silent refresh on reconnect
- leaves non-auth users on the current API and behavior

This should be an extension of `StdbPlugin`, not a separate top-level auth plugin.

---

## Design constraints

- `with_auth(...)` should augment `with_reconnect(...)` and `with_delayed_connection()`, not replace them.
- `with_token(...)` should remain valid and compatible with auth.
- PKCE and `state` verification should be mandatory.
- Interactive browser auth should happen only when explicitly requested or during startup gating.
- Reconnect should prefer silent recovery. It should not unexpectedly open the browser mid-session by default.
- Access tokens should stay in memory. Refresh token persistence should be optional.

---

## Recommended public API

### Plugin API

- `StdbPlugin::with_auth(options: StdbAuthOptions) -> Self`

### Auth options

Keep `StdbAuthOptions` as the public entry point, but extend it:

- `auth_endpoint: String`
- `token_endpoint: String`
- `client_id: String`
- `redirect_uri: String`
- `scopes: Vec<String>` or `Option<Vec<String>>`
- `startup_behavior: StdbAuthStartupBehavior`
- `storage: StdbTokenStorage`

Recommended startup behavior:

- `SilentFirst`  
  Try stored refresh token first, then fall back to interactive login.
- `Interactive`  
  Skip refresh-first startup and require browser login.

### Messages and state

These are a good fit for Bevy-side app control:

- `RequestLoginMessage`
- `RequestLogoutMessage`
- `AuthSuccessMessage(TokenResponse)`
- `AuthFailureMessage { message: String }`

For state, prefer this over `Completed`:

- `Unauthenticated`
- `Authenticating`
- `Authenticated`

`Completed` is ambiguous because both success and failure are “completed”.

### Token response

Your proposed `TokenResponse` shape is reasonable and should stay public if apps may need `scope`, `id_token`, or expiry metadata.

---

## Storage recommendation

For MVP, support:

- `None`
- `PlatformDefault`
- `Custom(...)`

The important part is that persistence is optional.

Recommended persistence behavior:

- persist `refresh_token`
- do not rely on persisted `access_token`
- keep the last successful `access_token` in memory for reconnect during the current session

This keeps the secure/default path simple while still enabling refresh-first startup.

If `Custom(...)` is added, keep the storage contract narrow:

- load token bundle
- save token bundle
- clear token bundle

---

## Lifecycle integration

### 1. Eager startup + auth

If the plugin is eager and auth is configured:

1. startup still requests a connection
2. auth intercepts that request before the connection is built
3. auth resolves an access token:
   - refresh-first if configured and possible
   - otherwise interactive browser login
4. once a valid access token exists, continue with the normal connection build

The startup is still “eager”, but the actual DB connection is gated on auth completion.

### 2. Delayed connection + auth

If `.with_delayed_connection()` and `.with_auth(...)` are both enabled:

- `StdbConnectionController::connect()` becomes auth-aware
- it should trigger the same token resolution flow before starting the DB connection
- `connect_with_token(...)` should still bypass the interactive flow and use the supplied token directly

### 3. Reconnect + auth

Reconnect should stay in the current subsystem, but become auth-aware:

1. reconnect first retries using the most recently stored in-memory access token
2. if that token is invalid and a refresh token exists, attempt silent refresh
3. if refresh succeeds, retry connect with the new access token
4. if refresh fails, emit auth failure and transition to `Unauthenticated`

Recommended default: do **not** open the browser from reconnect logic. Interactive login should be an explicit app action or startup fallback.

### 4. Logout

Logout should:

- clear in-memory auth state
- clear persisted refresh token
- disconnect the active `StdbConnection`, if any
- move auth state to `Unauthenticated`

If SpacetimeDB later exposes a revocation/logout endpoint, that can be added as a follow-up.

---

## Internal architecture

The cleanest fit is to add auth as a gate in front of connection creation.

### Suggested runtime pieces

- `StdbAuthConfig` resource  
  Derived from `StdbAuthOptions`.

- `StdbAuthState` state  
  Tracks `Unauthenticated` / `Authenticating` / `Authenticated`.

- `CurrentTokens` resource  
  Holds current in-memory `access_token`, optional `refresh_token`, and expiry metadata.

- `PendingAuthRequest` resource  
  Represents an in-flight login or refresh operation.

- `AuthAttemptContext` resource  
  Holds PKCE verifier, generated `state`, and redirect handling context for the current flow.

### Suggested system flow

1. a connection is requested
2. auth gate decides whether a valid token is already available
3. if yes, write token into connection config and proceed
4. if no, start refresh or interactive auth
5. when auth succeeds, store tokens and resume the queued connection request
6. existing connection code builds and activates the connection normally

This preserves the current connection ownership model instead of creating a parallel auth-owned connection path.

---

## Platform split

The current module split is correct:

- native implementation in `src/auth/native.rs`
- web implementation in `src/auth/web.rs`

Recommended responsibility split:

### `src/auth/mod.rs`

Shared public API and shared auth runtime:

- `StdbAuthOptions`
- startup behavior enum
- public messages
- token models
- auth resources/states
- shared systems
- shared token exchange / refresh orchestration

### `src/auth/native.rs`

Native-only flow:

- generate auth URL
- open system browser
- listen on local redirect URI
- parse callback query
- return `code` + `state`

### `src/auth/web.rs`

Web-only flow:

- generate auth URL
- redirect or open browser-compatible flow
- resume from callback URL in the page context
- parse callback query / fragment as needed
- return `code` + `state`

Keep the existing compile-time split for `wasm32` + `browser`.

---

## File-by-file plan

### `src/auth/mod.rs`

Expand this from simple options into the main shared auth module.

Add:

- finalized `StdbAuthOptions`
- `StdbTokenStorage`
- `TokenResponse`
- auth messages
- auth state enum
- token resources
- auth plugin/internal systems
- platform module dispatch

### `src/plugin.rs`

Implement `with_auth(options)` and wire auth into plugin build.

Changes:

- store `auth_options`
- insert auth plugin/resources when configured
- order auth systems before connection start systems
- keep current behavior unchanged when auth is not configured

### `src/connection.rs`

Make connection startup auth-aware without changing the external connection API.

Changes:

- gate eager/manual connection requests through auth when configured
- keep `connect_with_token(...)` as an explicit override
- update stored in-memory token on successful connect so reconnect uses the newest token

### `src/reconnect.rs`

Integrate silent auth recovery into reconnect.

Changes:

- retry with current in-memory token first
- attempt refresh before exhausting reconnect
- avoid interactive login by default inside reconnect

### `src/lib.rs`

Export new auth types through the prelude and update crate docs/examples.

---

## Implementation phases

### Phase 1: public API and shared runtime

- finalize `StdbAuthOptions`
- add auth messages/state
- implement `with_auth(options)`
- add auth resources and system ordering
- no browser flow yet

Done when the crate can compile with auth configured and the runtime shape is stable.

### Phase 2: interactive login flow

- native browser open + local callback listener
- web callback resume flow
- PKCE + `state` generation and verification
- code exchange at `/token`

Done when explicit login can produce an access token on native and web.

### Phase 3: startup gating

- make eager startup auth-aware
- make delayed connection auth-aware
- resume pending connect after successful auth

Done when `.with_auth(...)` works with both eager and delayed connection modes.

### Phase 4: token persistence and refresh-first startup

- add optional storage
- load stored refresh token at startup
- attempt silent refresh before browser login
- persist updated refresh token after success

Done when later launches can connect without reopening the browser.

### Phase 5: reconnect integration

- retry with latest in-memory access token
- refresh and retry on auth failure
- move to `Unauthenticated` on silent auth exhaustion

Done when auth and reconnect work together without app-side glue.

### Phase 6: docs, examples, tests

- auth example app
- native and web usage docs
- tests for state verification, callback parsing, token storage, and reconnect behavior

---

## Recommended behavioral decisions

### Prefer `Authenticated` over `Completed`

`Completed` is not a useful long-term public auth state. Use `Authenticated`.

### Do not auto-open browser during reconnect

This is the safest default for games and avoids surprising browser launches after disconnects.

### Keep `with_token(...)` compatible

If both `with_token(...)` and `with_auth(...)` are set:

- use the provided token as the initial in-memory access token
- still allow refresh/logout/auth-aware reconnect behavior if auth is configured

### Start with fixed redirect URI support

Support the configured `redirect_uri` first. Dynamic-port native redirect URIs can be a follow-up if needed.

---

## Acceptance criteria

- `with_auth(...)` exists and integrates with `StdbPlugin`
- eager startup waits for auth before connecting
- delayed connection waits for auth before connecting
- explicit login works on native and web
- returned `state` is verified
- token exchange uses PKCE
- refresh-first startup works when storage is enabled
- reconnect can silently refresh and retry
- logout clears persisted and in-memory auth data
- existing non-auth users keep the current behavior unchanged

---

## Relevant existing code

These are the current integration points this plan is based on:

- `Projects/bevy_stdb/src/auth/mod.rs#L1-L15`
- `Projects/bevy_stdb/src/plugin.rs#L57-L67`
- `Projects/bevy_stdb/src/plugin.rs#L403-L458`
- `Projects/bevy_stdb/src/plugin.rs#L496-L498`
- `Projects/bevy_stdb/src/connection.rs#L145-L248`
- `Projects/bevy_stdb/src/connection.rs#L270-L292`
- `Projects/bevy_stdb/src/connection.rs#L490-L524`
- `Projects/bevy_stdb/src/reconnect.rs#L1-L194`

# 0027: The connectors crate

## Context

Clauseo sells optional paid data connectors that it runs itself at `data.clauseo.chat`. The first
is case law (`case-law`). The app has to sign the lawyer in to that connector, keep the credential,
refresh it, revoke it, read the account, start a recharge and wait for it, and tell whether the
connector is live and what it costs. The relay that gives the agents access (R24) needs a bearer
token on demand, and the agents never hold the credential.

The core crates do no network I/O and have no async runtime, and Tauri stays out of them. The
credential is a secret that must not reach settings, logs, errors or crash reports. Windows
Credential Manager caps an item at 2,560 bytes.

## Decision

A new crate, `crates/connectors` (package `clauseo-connectors`), does all of it with blocking I/O
and no async runtime. It depends on no Clauseo crate; R24 maps its types into `protocol`. What it
decided, as applied:

- **One HTTP agent**: ureq over rustls with the `ring` provider and the OS trust store
  (rustls-platform-verifier). Connect timeout 10 s, whole request 60 s, 50 s for a `wait=40`
  recharge poll. Redirects are never followed. Production and staging are `https` only. The proxy
  is `ProxyChoice::Env` until S16's upstream lookup exists.
- **Origins.** Every compiled-in URL starts with `https://data.clauseo.chat`. Under `Staging` and
  the test-only `Fake` (`http://127.0.0.1:<port>` only), that prefix maps onto the origin and the
  path is kept. The discovery, PKCE, `state` and `iss` checks stay on in every origin.
- **Sign-in.** Authorization code with PKCE S256 and `state`, through the system browser and a
  one-shot loopback listener on `127.0.0.1` (with `SO_EXCLUSIVEADDRUSE` on Windows; socket2 0.6.5,
  the version in the lock, has no safe setter for that option, so `net/windows.rs` calls Winsock
  itself). Each connection gets one total deadline of 2 s of real time for its whole request
  head. The accept loop takes at most 16 connections per tick and checks the cancel flag and the
  timeout every tick. At most 8 heads are read at once, and later connections wait in a short
  queue, so slow local clients delay the callback but can't drop it. Discovery
  checks the protected resource and authorization server metadata, and every endpoint must sit on
  the issuer's scheme, host and port. The callback must carry the right `state`: a wrong or
  missing one gets 400 and the listener keeps waiting. Then the callback must carry the issuer as
  `iss` (RFC 9207). The browser is answered only after the exchange and the keychain write: the
  fixed success page (`src/oauth/page.html`), or on any failure a fixed 400 page
  (`src/oauth/failed.html`). Both are English, then Hindi, with no script.
- **The credential** is one keychain item per connector (`ai.clauseo.desktop.connectors`, account
  `<connector id>`), a small JSON under 2,000 bytes, read once per process and then held in memory.
  The state file (`connectors/state.json`) holds no secret, only which connectors were added and
  how their credential ended.
- **Refresh is single-flight**: the credential's lock is held while it runs. It starts 5 minutes
  before expiry, or after a 401 when the token used is still the current one. A rotated refresh
  token is written to the keychain before the new access token is used. A failed write is
  retried once, and then `persist_failed()` turns true. `invalid_grant`, or a 401 after the
  refresh, deletes the item and records `signed_out`. Just before each refresh, the keychain item
  is read again under the lock, so two instances (two processes, say) don't fork the chain. If
  the item is gone, the connector is signed out. If its refresh token differs from the one in
  memory, the stored credential is adopted. Its access token is returned without a refresh when
  more than 5 minutes are left; otherwise the refresh uses the stored refresh token. While
  `persist_failed()` is true, memory holds the newer token and is kept.
- **A new sign-in or `add_api_key` that replaces an OAuth credential** first revokes the old
  refresh token, best effort. A failure is logged without the token and doesn't block the new
  credential.
- **Sign out** is `POST /revoke` for the refresh token, then the keychain delete, then memory. An
  API key has no revoke call. **Remove** signs out and forgets the connector.
- **Liveness** is `pricing.json` with `status: "live"`, cached 24 hours, or 10 minutes after a
  failure, and never polled. **The account** is read at most once a minute
  (`ACCOUNT_MIN_INTERVAL`). A pay link leaves the crate only as a `PayUrl`: exactly
  `https://data.clauseo.chat/pay/<id>`, `<id>` in `[A-Za-z0-9_-]{1,128}`, with no port (not even
  an explicit `:443`), user info, query, fragment, backslash or percent escape. The recharge poll
  starts polls at least 2 s apart, waiting on the cancel flag, and an answer for another
  `recharge_id` is `Error::Parse`.
- **Secrets** are `Secret` values: no `Display`, no `Serialize`, `Debug` prints `Secret(..)`, and
  the bytes are zeroed on drop. Errors name the kind of failure or a status, never a token, a code,
  a URL query or a response body. A test runs a whole sign-in under a capturing subscriber with
  `LogTracer` and finds none of the fake's secrets in the logs or in any public value's `Debug`.

ARCHITECTURE's invariant "Clauseo never reads a credential" is about the agents' own logins
(Claude Code, Codex). This credential belongs to Clauseo, which holds it for its own connector.
The relay's decision record (R24) states that exception for the relay's side.

### Deviations from the S28 brief

- One change, not two: the API and its bodies land together, because the workspace lints deny
  `todo!()`.
- Additions to the API, for tests and origins: `PayUrl::parse_for`, `classify_tool_result_for`,
  `Error::BadOrigin`, `KeyringStore::new`, `MAX_SECRET_BYTES`, `ACCOUNT_MIN_INTERVAL` (public),
  and three hidden test-support hooks: `ConnectorHandle::seed_rng_for_test`,
  `PendingSignIn::tamper_verifier_for_test` and `Connectors::set_request_timeout_for_test`.
- `SignInError::Busy` is never returned: a second `begin_sign_in` cancels the first, as the brief
  says. A listener that cannot open is `SignInError::Exchange`, since there is no I/O variant.
- `mcp_post` without a credential, or after the credential ended, sends no `Authorization`, and a
  401 from that call comes back as the reply. `add_api_key` stores a key that the account call
  answers with 402, because a valid key on an empty account is still valid.
- `wait_recharge` also backs off on a 5xx, and runs each poll on its own thread, so the cancel
  flag and the deadline take effect within about 20 ms instead of at the end of a 40 s poll.
- The live tests use the `live_` prefix and `CONNECTORS_LIVE_ORIGIN` (`staging` or
  `fake:<loopback url>`) instead of `staging_` and `CONNECTORS_STAGING=1`.
- The ARCHITECTURE row's status is `exists`.
- **A refresh that fails on the network keeps a token that hasn't expired.** The brief's rule
  says a failed refresh is an error. Instead, when a refresh started ahead of expiry fails on the
  network and the current access token has not expired yet, `bearer` returns the current token
  and logs a warning with no secret. Once the token has expired, the failure is
  `AuthError::Network`. A refresh after a 401 never falls back, since the token was just refused.
- A clock overflow (`SystemTime` plus a configured duration) is `Error::Parse`. `SignInError`
  has no `Parse` variant, so a sign-in timeout that overflows is `SignInError::Exchange`.
- The test-support build also has `ConnectorHandle::listeners_bound_for_test`, and the fake has
  `answer_next` and `close_next`.

### For R24

- Open the authorize URL without putting it on any process's argv where the OS allows (not
  `open <url>` or a `ShellExecute` command line). Another local process could read the `state`
  and the PKCE challenge there, run the authorize step as its own account, and sign the lawyer in
  to that account.
- A body an agent controls that draws a 401 twice signs the lawyer out (a 401 after a refresh
  means signed out). The relay should treat a 401 after a refresh on `tools/call` with care
  rather than pass every such body straight through.
- `status()` and `bearer()` take the credential's lock, which a refresh holds for up to 60 s.
  Keep them off the UI thread.

## Consequences

R24 builds the relay on `bearer` and `mcp_post` and never sees the refresh token. The fake connector
(`test-support`) lets R24's tests run with no network. Each new connector is a new compiled-in
`KnownConnector`; the app never offers an unknown one. The words of the success and failure pages
wait for Rohan's copy review. A later change must keep the credential out of every log, error and file other than
the keychain item, and must keep the refresh single-flight, or a rotation forks the chain and the
connector revokes it.

## Status

proposed

## Date

2026-11-02

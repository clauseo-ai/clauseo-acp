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
  one-shot loopback listener on `127.0.0.1` (with `SO_EXCLUSIVEADDRUSE` on Windows). Discovery
  checks the protected resource and authorization server metadata, and every endpoint must sit on
  the issuer's scheme, host and port. The callback must carry the right `state`: a wrong or
  missing one gets 400 and the listener keeps waiting. Then the callback must carry the issuer as
  `iss` (RFC 9207). The fixed success page is English, then Hindi, in `src/oauth/page.html`.
- **The credential** is one keychain item per connector (`ai.clauseo.desktop.connectors`, account
  `<connector id>`), a small JSON under 2,000 bytes, read once per process and then held in memory.
  The state file (`connectors/state.json`) holds no secret, only which connectors were added and
  how their credential ended.
- **Refresh is single-flight**: the credential's lock is held while it runs. It starts 5 minutes
  before expiry, or after a 401 when the token used is still the current one. A rotated refresh
  token is written to the keychain before the new access token is used. A failed write is
  retried once, and then `persist_failed()` turns true. `invalid_grant`, or a 401 after the
  refresh, deletes the item and records `signed_out`.
- **Sign out** is `POST /revoke` for the refresh token, then the keychain delete, then memory. An
  API key has no revoke call. **Remove** signs out and forgets the connector.
- **Liveness** is `pricing.json` with `status: "live"`, cached 24 hours, or 10 minutes after a
  failure, and never polled. **The account** is read at most once a minute
  (`ACCOUNT_MIN_INTERVAL`). A pay link leaves the crate only as a `PayUrl` on the connector's own
  `/pay/` path.
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
- The ARCHITECTURE row's status is `exists`. The success page answers an `error` or a bad `iss`
  with an empty 400 instead of a page.

## Consequences

R24 builds the relay on `bearer` and `mcp_post` and never sees the refresh token. The fake connector
(`test-support`) lets R24's tests run with no network. Each new connector is a new compiled-in
`KnownConnector`; the app never offers an unknown one. The success page's words wait for Rohan's
copy review. A later change must keep the credential out of every log, error and file other than
the keychain item, and must keep the refresh single-flight, or a rotation forks the chain and the
connector revokes it.

## Status

proposed

## Date

2026-11-02

# 0013: codex_appserver

## Context

The usage reader (S13) asks a Codex app-server for the account's plan and rate limits: it sends
`initialize`, `initialized`, `account/read` and `account/rateLimits/read` with
`excludeResetCreditDetails: true`. The direct Codex driver planned for after the beta speaks the
same protocol. Three constraints shape the crate that holds it:

- No Codex crate. Walking Codex's lockfile from `codex-app-server-protocol` reaches 868 of 1,466
  packages, and crates.io has only a third-party fork. So the types are ours, written against
  the JSON schema Codex commits at `codex-rs/app-server-protocol/schema/json`.
- The reader runs the Codex codex-acp bundles, `@openai/codex` 0.162.1, so the schema comes from
  tag `rust-v0.162.1`.
- Codex adds values often: `PlanType` has 18 today. Generated types (typify) close every enum,
  so one new plan would fail the whole read.

The crate needs `serde` and `serde_json` for the messages, `futures` for the pipe traits, the
channels and the oneshot behind each call, and `tracing` for the debug lines on skipped messages.

## Decision

- A new crate, `clauseo-codex_appserver`. Its `ARCHITECTURE.md` row: Owns "Types for the pinned
  Codex app-server JSON schema, a minimal JSON-RPC client over any pipe pair, the schema drift
  test; reused by the direct Codex driver after the beta". May depend on `protocol, serde,
  serde_json, futures, tracing`. Never depends on `tauri, tokio`.
- **Hand-written types, not typify output.** Each enum keeps an unknown value in an `Unknown`
  variant that holds the string. Where the schema itself lists `"unknown"` (`PlanType`,
  `RateLimitResetCreditStatus`, `RateLimitResetType`), that value is the named variant
  `CodexUnknown`. Unknown fields are ignored. No codegen step, no `build.rs`.
- **What is mirrored**, named as in the schema: `InitializeParams` with `ClientInfo` and
  `InitializeCapabilities`, `InitializeResponse`, the `initialized` notification (a method name
  with no params), `GetAccountParams`, `GetAccountResponse` with `Account` (`ApiKey`,
  `ChatgptAccount`, `AmazonBedrockAccount`), `GetAccountRateLimitsParams`,
  `GetAccountRateLimitsResponse`, `RateLimitSnapshot`, `RateLimitWindow`, `CreditsSnapshot`,
  `SpendControlLimitSnapshot`, `RateLimitResetCreditsSummary`, `RateLimitResetCredit`,
  `PlanType`, `RateLimitReachedType`, `RateLimitResetCreditStatus`, `RateLimitResetType`, and
  `AccountRateLimitsUpdatedNotification`. Each takes every field its definition gives.
- **The vendored schema.** `crates/codex_appserver/schema/rust-v0.162.1/` holds the eight files
  the drift test reads, byte for byte. `schema/README.md` gives the source, each SHA-256, what
  was checked at the tag and the refresh steps. A pin bump is a new folder, a green drift test,
  and the old folder removed.
- **The drift test** (`tests/schema_drift.rs`) checks each mirrored type against its definition:
  field names as serde reads them, required against optional (by decoding a sample built from
  the schema with each field removed), `null` where the schema allows it, and every enum value
  the schema lists having a named variant. A definition in a vendored file that is neither
  mirrored nor listed as skipped fails it, and so does a fixture that does not fit its
  definition. The checker has its own tests on a hand-made schema.
- **The line format.** One JSON message per line, at most 8 MiB, as `acp` caps an agent's
  lines. Written messages carry no `"jsonrpc"` member: the schema's JSON-RPC types have none, and
  upstream's `serialize_get_account_rate_limits` test shows that wire form. Read messages may
  carry it or not. A line that is not a JSON-RPC message, or one over 8 MiB, ends the connection
  with `Error::Protocol`.
- **The interface** S13 builds on: `connect(writer, reader) -> (Client, Notifications, Driver)`.
  `Client` is `Clone + Send`, with `initialize`, `account` and `rate_limits`. `Notifications` is
  an unbounded stream of `RateLimitsUpdated(..)` or `Other { method }`. `Driver` is the future
  the caller runs. The crate spawns nothing and has no executor, clock or sleep.
- `initialize` sends `clientInfo` `{ name: "clauseo", version: <crate version> }` and empty
  `capabilities`. At the tag `account/read`, `account/rateLimits/read` and
  `account/rateLimits/updated` carry no `#[experimental]` mark, so the client never sets
  `experimentalApi`.
- One `Error`: `Closed` maps to `agent.exited`; `Rpc { code, message }` and `Protocol` map to
  `agent.protocol_error`.

## Consequences

- A Codex release that adds a plan, a limit type or an account kind decodes; the reader sees
  `Unknown("…")` until a pin bump names it. A pin bump that renames or drops a field fails the
  drift test, not a lawyer's read.
- The types cover only the reader's calls. Thread, turn, item and approval types wait for the
  direct driver, which may switch to generated types then; the drift test's approach carries
  over.
- The crate never starts `codex`: S13 spawns it through `process` and hands over the pipes, and
  owns every timeout. The caller starts `codex app-server` with Clauseo's own `CODEX_HOME` (S13's
  reader takes it as an input; #160); this crate never reads `~/.codex` and never chooses a home. The client was tested against a scripted peer only; the first run against
  a live `codex app-server` is G16's, through S13.
- `Notification::Other` carries every notification but `account/rateLimits/updated`, params
  dropped, so a caller can react to `account/updated` by name without this crate decoding it.

## Status

accepted

## Date

2026-11-02

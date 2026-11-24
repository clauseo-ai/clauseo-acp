# Vendored Codex app-server schema

`rust-v0.171.0/` holds the files `tests/schema_drift.rs` reads, copied byte for byte from
openai/codex at tag `rust-v0.171.0` (tag object `295eca93`, commit
`ccf9be8cc458bd6b8336fe84d68d8893f2691a5e`), path `codex-rs/app-server-protocol/schema/json`:
<https://github.com/openai/codex/tree/rust-v0.171.0/codex-rs/app-server-protocol/schema/json>.
That is the Codex codex-acp bundles (`@openai/codex` 0.171.0). The path holds 37 files and the
folders `v1/` and `v2/` at that tag; the subfolders here keep upstream's layout.

| File | SHA-256 |
|---|---|
| `ClientNotification.json` | `706cf248d75027c84a3c63348d0ed507182e8eba40069dd17541793de029145a` |
| `v1/InitializeParams.json` | `c608ff6bdd94e661423dca9b44cd1dcea7eac610f74e1310284c61eab64280bc` |
| `v1/InitializeResponse.json` | `62ad689c2cb6379913c1d72749cfd8de5089d35760214123518eb92eef11acc9` |
| `v2/AccountRateLimitsUpdatedNotification.json` | `fb62719b2edf989399a744e06e38b1828bf81bd10afaf6910d91c39b2e48d952` |
| `v2/GetAccountParams.json` | `30b545275f60975b55bd6fbaafd70f15505801d14168ee82d3931b6e93c78aab` |
| `v2/GetAccountRateLimitsResponse.json` | `9c0def7f8ab2396122af939dffbf01b34b8d66082fe4129dd20917bfd4313fdc` |
| `v2/GetAccountResponse.json` | `588bac1dacbcf8d0676f2c31d9d5750e8baaee5c3062ef820ae4e6ee68687418` |
| `v2/ModelListParams.json` | `3f48937b1e0348ede28d539011c571909fd79cb9f05d2343fe484839633cfdf7` |
| `v2/ModelListResponse.json` | `2fb387a2ed258fc74258828ea342d68f89ce4e8e22cd0329ca59d2b423f9108b` |
| `v2/NullableGetAccountRateLimitsParams.json` | `072f813ca1c696831a7f2fbd934feaa8cad228a6b3d44a4be8877547c796341b` |

`GetAccountRateLimitsParams` has no file of its own at the tag: it is defined in
`v2/NullableGetAccountRateLimitsParams.json`, because `account/rateLimits/read` takes params or
none.

## What was checked at the tag (2026-11-24)

- **Method names.** `ClientRequest.json` names `initialize` (params `InitializeParams`),
  `account/read` (params `GetAccountParams`, required) and `account/rateLimits/read` (params
  `GetAccountRateLimitsParams` or `null`, optional). `ServerNotification.json` names
  `account/rateLimits/updated` (params `AccountRateLimitsUpdatedNotification`).
  `ClientNotification.json` names `initialized`, with no params.
- **Not experimental.** In `codex-rs/app-server-protocol/src/protocol/common.rs`, experimental
  methods carry `#[experimental("...")]` before their entry. `GetAccountRateLimits =>
  "account/rateLimits/read"` (line 1472) and `GetAccount => "account/read"` (line 1612) carry
  none, and neither does `AccountRateLimitsUpdated => "account/rateLimits/updated"` (line 2153).
  So the client never sets `capabilities.experimentalApi`. The schema in this folder is the
  stable form: `account/read`'s experimental `workspaceRouting` field is left out of
  `GetAccountResponse`, though its `WorkspaceRouting` definition remains.
- **Envelope.** `codex-rs/app-server/README.md` at the tag does not describe the envelope. The
  schema's `JSONRPCRequest`, `JSONRPCNotification`, `JSONRPCResponse` and `JSONRPCError` have no
  `jsonrpc` property, and upstream's test `serialize_get_account_rate_limits` (`common.rs`)
  shows the wire form `{"method": "account/rateLimits/read", "id": 1}`. The client writes no
  `jsonrpc` member and reads messages with or without one.

`model/list` (params `ModelListParams`, response `ModelListResponse`) was added for the host's
model probe on 2026-11-24. It is not experimental at the tag; the probe sends only `cursor`.

## Refreshing on a pin bump

1. In a temporary folder outside the repo, fetch each file above at the new tag, for example
   `curl -fsSL -o v2/GetAccountResponse.json https://raw.githubusercontent.com/openai/codex/<tag>/codex-rs/app-server-protocol/schema/json/v2/GetAccountResponse.json`.
   List the folder first (`gh api "repos/openai/codex/contents/codex-rs/app-server-protocol/schema/json?ref=<tag>"`)
   to confirm the path and file names still hold.
2. Recheck the three points above against the new tag's `common.rs`, `ClientRequest.json`,
   `ServerNotification.json` and `ClientNotification.json`.
3. Copy the files into a new folder `schema/<tag>/`, set `SCHEMA_TAG` in
   `tests/schema_drift/checker.rs` to the tag, and remove the old folder.
4. Run `cargo nextest run -p clauseo-codex_appserver`. Fix every type the drift test names
   (a new enum value needs a named variant), and every fixture `fixtures_match_the_schema` names.
5. Replace the tag, commit and hashes here (`shasum -a 256` on each file), and delete the
   temporary folder.

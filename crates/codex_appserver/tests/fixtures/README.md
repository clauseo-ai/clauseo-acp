# Fixtures

Each file is the `result` (or, for a notification, the `params`) that `tests/client.rs`'s scripted
app-server sends, written by hand from the vendored schema in `../../schema/<tag>/` (the tag is
`SCHEMA_TAG` in `../schema_drift/checker.rs`). `fixtures_match_the_schema` in
`tests/schema_drift.rs` reads the table below and checks each fixture against the definition named
for it: types, required fields, enum values, and no field the schema does not name. Every `.json`
file here must have a row.

| Fixture | Schema file | Definition |
|---|---|---|
| `initialize_response.json` | `v1/InitializeResponse.json` | `InitializeResponse`, the `initialize` response |
| `account_response_chatgpt.json` | `v2/GetAccountResponse.json` | `GetAccountResponse`, the `account/read` response, with a `ChatgptAccount` |
| `rate_limits_response.json` | `v2/GetAccountRateLimitsResponse.json` | `GetAccountRateLimitsResponse`, the `account/rateLimits/read` response |
| `rate_limits_updated.json` | `v2/AccountRateLimitsUpdatedNotification.json` | `AccountRateLimitsUpdatedNotification`, the `account/rateLimits/updated` params |

Each definition is the root of its schema file. The fixtures hold made-up values; recordings of a
live `codex app-server` belong to the usage reader's fixtures, not here.

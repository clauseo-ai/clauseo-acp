# Connector fixtures

The case-law connector (clauseo-connectors, private) is not built yet, so every file here is
written by hand from the connector thread's answers of 2026-11-02 and the connector's design. None
is a recording. Done-when 4 of S28 replaces each one with a recording from staging, secrets
removed; if a recorded shape differs, the fixture and its parser change together and the PR says
so.

Every URL is written with the production origin, `https://data.clauseo.chat`. The fake connector
(`src/fake.rs`) serves these files with that origin replaced by its own
`http://127.0.0.1:<port>`, and fills in what changes per run (tokens, codes, the balance, recharge
ids).

| File | Shape from | Notes |
|---|---|---|
| `prm-case-law.json` | RFC 9728, the thread's discovery answer | `resource` is the MCP URL; the issuer is the one authorization server |
| `as-metadata.json` | RFC 8414, the thread's discovery answer | `client_id_metadata_document_supported` and `authorization_response_iss_parameter_supported` true; auth method `none`; `S256` |
| `client-metadata.json` | the thread's client ID document | served at `/oauth/clients/clauseo-desktop.json` |
| `token.json`, `token-rotated.json` | RFC 6749 section 5.1 | the token values are placeholders; the fake makes its own |
| `token-invalid-grant.json` | RFC 6749 section 5.2 | |
| `pricing-live.json`, `pricing-preview.json` | the thread's `pricing.json` answer | packs 200 to 5000 at 0.10 INR a credit plus 18% GST: **examples, not prices** |
| `account.json` | the thread's account answer | `limits` and `brake` are not read by the crate |
| `account-open-payment.json` | the same, with an open payment | `open_payment` has the payment object's shape |
| `account-unknown-fields.json` | `account.json` with fields added at every level | must parse to the same values as `account.json` |
| `recharge-created.json`, `recharge-paid.json` | the payment object of the out-of-credit answer | **unverified**: the recharge answer's shape is still asked. The crate reads the object itself or under a `payment` key |
| `api-402.json` | the thread's out-of-credit answer | the JSON API's body with HTTP 402 |
| `mcp/initialize.json` | MCP `initialize` result | the fake also answers `server/discover` with it |
| `mcp/tools-list.json` | the thread's six tools and their annotations | input schemas are sketches |
| `mcp/search-law-result.json` | **unverified** | titles and links in `structuredContent.results[]` as `{title, url}`, pending the thread's answer |
| `mcp/read-document-result.json` | **unverified** | `structuredContent.documents[]` as `{title, url}`, pending the thread's answer |
| `mcp/insufficient-credits-result.json` | the thread's out-of-credit answer | `isError: true`; `structuredContent` is `api-402.json`'s body |

The Indian Kanoon links in the two result fixtures are examples; nothing reads them but tests.

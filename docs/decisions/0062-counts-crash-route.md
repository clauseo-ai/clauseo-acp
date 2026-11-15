# 0062: The crash route

## Context

Decision 0060 (`0060-crash-reports-launch-cut.md`, which lands with the app's crash pull
request) cuts S22's crash reports for launch to one small, closed `CrashReport` in
`crates/counts`. It holds no install or tally id, no time finer than
the ISO week, no message, and no path but a source file in two fixed forms. The types are read
through `parse_crash`, with 2 KB caps compressed and inflated. Task S23 gives the Worker
`POST /v1/crash`, R2 in the EU, a `crash` table, `v_crash`, and at most 200 reports an hour.
The Worker runs on Workers Free, and R2 needs a card on file before a bucket can exist, so the
deploy must work with no bucket. This record covers `migrations/0004_crash.sql`. It does not
depend on 0003 (the dashboard's pull request); D1 applies migrations by name.

## Decision

- **The route.** `POST /v1/crash` follows the counts routes' steps: gzip only; caps of 2 KB
  gzip and 2 KB inflated (the crate's `MAX_CRASH_GZIP` and `MAX_CRASH_JSON`); the crate's
  `parse_crash` with this week from the Worker's clock (`400` on anything it refuses, an
  `Invalid::Detail` too). Then:
  1. `503` while the R2 binding `CRASH` is missing.
  2. The hourly cap: `crash_rate` holds only an hour (`YYYY-MM-DDTHH`, UTC) and its count. Each
     report first deletes past hours, then adds one unless the hour is at 200
     (`… DO UPDATE SET n = n + 1 WHERE n < 200 RETURNING n`); no row back is `429` and changes
     nothing.
  3. The object: `crash_body`'s bytes (the crate's canonical JSON, never the request's bytes)
     at `reports/<report id>.json`.
  4. The row, `INSERT OR IGNORE`, keyed by the report id.

  The object goes before the row, so a failed insert is retried by the app and rewrites the
  same object. A report sent again counts against the hour, rewrites its object and leaves its
  row.
- **Columns.** `crash` (`WITHOUT ROWID`): `report`, `week`, `version`, `os`, `arch`, `kind`,
  `area`, `signature`. The signature is made only from closed fields: a panic's `file:line`
  (or `panic` when the file fit neither form), a signal's `code:<number>`, a renderer's reason.
  S23's `frames` column is not here: the launch cut has no frames, and the object holds the
  whole report.
- **`v_crash`.** Reports per week, version, OS, arch, kind, area and signature. A second set of
  rows, with signature `_all`, groups the same without the signature, so a kind spread over many
  small signatures still shows. Every group under 5 reports is hidden. There is no `report`
  column.
- **Retention.** The daily cron deletes `crash` rows whose week is more than 365 days old (one
  statement; 37 a run now). An R2 lifecycle rule deletes objects 90 days after upload. R2 keeps
  each object's upload time, which shows roughly when a report arrived; S22's random delay of 1
  to 24 hours before sending is the guard.
- **The deploy.** `scripts/r2.mjs`, run in the config step through Cloudflare's API:
  - It finds `clauseo-crash` with `cf-r2-jurisdiction: eu`, or creates it there.
  - It reads the lifecycle and, unless our rule (`clauseo-crash-90-days`, delete at 90 days,
    every prefix) is there as written, writes it beside the other rules.
  - On `401` or `403` (the token has no Workers R2 Storage Edit) it skips with one line.
  - The deploy copy keeps the `[[r2_buckets]]` table only when the bucket is there, so a deploy
    before R2 exists still deploys and the route answers `503`.
  - `check-deploy.mjs` requires that exact table in the committed file and allows it, exact or
    absent, in the deploy copy.
  - Nothing printed holds an id.
- **Free plan.** A 2 KB report costs well under 1 ms of CPU. A report is 4 D1 statements (2
  writes and a delete in `crash_rate`, then 1 insert) and one R2 put. At 200 an hour that is at
  most 4,800 puts a day, inside R2's free Class A operations.

## Consequences

- `/v1/crash` answers `503` until Rohan adds R2 to the account and Workers R2 Storage Edit to
  the deploy token. The app keeps reports for 30 days and retries.
- The hourly cap is global, so a flood holds back everyone's reports for the rest of the hour.
  The app retries a `429`.
- Where this and the dashboard's pull request touch the same lines (`route.rs`, `lib.rs`,
  `deploy.mjs`, the docs, the cron's statement count), the one merged second resolves them.
- Symbolication, frames and `clauseo-symbols` wait for full S22.

## Status

proposed (Rohan confirms in review)

## Date

2026-11-15

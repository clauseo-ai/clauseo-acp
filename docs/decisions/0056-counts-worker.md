# 0056: The counts Worker, launch cut

## Context

Task S23 designs one Cloudflare Worker for Clauseo's telemetry. It takes the day reports and
weekly tallies of [decision 0036](0036-counts.md), the crash reports of S22 and the release job's
version rows. It also serves Rohan a dashboard behind Cloudflare Access and agents a read-only
SQL endpoint. For launch only the counts must arrive, and S22's crash reports are not built yet.
The orchestrator approved a launch cut: the two counts routes, D1, global daily caps, the
retention cron, the privacy guards, the two workflows and the docs. The rest of S23 lands after
launch. The account runs on Workers Free for now. Free allows 10 ms of CPU and 50 D1 queries per
invocation, 100,000 rows written and 5 million read a day, and 500 MB per database, and it keeps
D1 Time Travel for 7 days.

## Decision

`services/counts-worker/` is a Rust Worker on `workers-rs`, built with `worker-build`. Like
`fuzz/`, it has its own `[workspace]` and `Cargo.lock`, so the root workspace never builds it. It
uses the root's `rust-toolchain.toml` and adds the `wasm32-unknown-unknown` target. It depends on
`crates/counts` with `default-features = false`, which builds for wasm32 unchanged. It validates
only through `parse_batch` and `parse_tally` and the crate's size constants. No list of allowed
values is copied anywhere, and both workflows run the crate's schema snapshot test before anything
else. `docs/counts-worker.md` describes what is accepted, stored and kept, and how it deploys.

- **Routes.** `POST /v1/counts` and `POST /v1/tally`. Any other path gets `404`; another method on
  those two gets `405`. The steps run in order: path, headers, a read capped at the gzip size, a
  capped inflate, the crate's parser, the daily count, the insert. A refused request never
  touches D1. A missing or empty body is `400`.
- **Every refusal of the body is `400`, a body over the gzip cap too.** The app's sender
  (`crates/counts/src/sender.rs`, `answered`) treats `2xx` and `400` as final and retries anything
  else. A `413` for a body the app built too large would be retried with the same body until the
  report aged out.
- **Columns.** `report` and `tally` hold each closed top-level field of `DayReport` and
  `TallyReport`, in its wire form, plus `cells` as the crate re-serialises it. `schema` is not a
  column, as `0001_init.sql` holds schema 1 only. Enum columns hold the crate's wire names, so a
  new value needs no migration. Nothing records the time a row arrived, the sender's address or
  any header.
- **Global daily caps in D1, with no client key.** Each request that passes the parser adds one to
  its route's row for the UTC day in a `rate` table, but only while the count is under the cap
  (`INSERT … ON CONFLICT (route, day) DO UPDATE SET n = n + 1 WHERE n < ?cap RETURNING n`). No row
  back means `429`, and nothing is written, the count included. The caps are 3,000 counts requests
  and 1,000 tallies a day. They are per day, not per hour, because the app sends within minutes of
  UTC midnight and an hourly cap would refuse that normal burst. The app retries a `429` later.
  - Writes at both caps: 3,000 × 8 (7 reports and the count) + 1,000 × 2 ≈ 26,000 rows a day.
    The cron's deletes add up to about 22,000 once rows expire. That is under 48,000 of Free's
    100,000.
  - No index beyond the keys, as each would add a written row to every insert. The cron's scan
    reads each table about once a day: at most about 1.9 million `report` rows at the caps,
    under Free's 5 million.
  - Storage is the open risk. Realistic reports (about 1 KB a day) add about 21 MB a day at the
    caps. A deliberate flood of the largest valid bodies could add about 192 MB a day and fill
    Free's 500 MB within days. Inserts then fail until rows expire or are deleted by hand.
- **A custom domain.** `routes = [{ pattern = "t.clauseo.chat", custom_domain = true }]`.
  Cloudflare makes the DNS record and certificate when the Worker deploys, so the S23 route
  `t.clauseo.chat/*` and a DNS record made by hand are not needed.
- **Retention.** A pure-SQL cron runs daily at 03:00 UTC, not weekly. It deletes `report` rows
  older than 90 days (gone at most 91 days after their day), `tally` rows whose week is older than
  396 days (gone at most 403 days after the week's Monday), and `rate` rows of past days. The
  Worker computes the cut-offs with the crate's `Day` and `Week`. The deletes go by fixed ranges
  of the key's first hex digit (`0` to `f`, the last range ending below `g`; the crate stores
  lowercase UUID v4 ids, which spread evenly): 16 for `report`, 16 for `tally` and one for
  `rate`, 33 statements under the 40-statement budget. No statement returns rows and none has a
  `LIMIT`, so the Worker decodes nothing per row, and one run clears the whole backlog. Time
  Travel adds 7 days on Workers Free, not S23's 30 on Paid.
- **The account ID.** Wrangler uses the one account the token sees. An optional environment
  secret `CLOUDFLARE_ACCOUNT_ID` is for a token that sees more; GitHub masks secrets, and an empty
  one is dropped before wrangler runs. No step runs `wrangler whoami`.
- **Public run logs.** Every wrangler call goes through `scripts/deploy.mjs`. It prints wrangler's
  output only through a redactor (UUIDs, 32-hex ids, emails, "…'s Account" names) and masks the
  database id with `::add-mask::`. No `env:` value carries an id. There is no input-based id
  check. The `check` action takes no input: it counts the rows of two fixed canary ids
  (`c0ffee00-0000-4000-8000-000000000001` for the install, `…0002` for the tally), prints the two
  counts and deletes those rows. The live check gives a test machine the canary install id in
  `<data_dir>/counts/install_id`, which the app reads and keeps (`read_install`,
  `crates/counts/src/store.rs`), and sends the canary tally by hand.
- **No token while building.** The wasm is built in a step with no secret. The deploy copy
  `wrangler.deploy.toml` has no `[build]`, so `wrangler deploy` uploads that output and runs no
  build with the token present (checked with `wrangler deploy --dry-run`). The deploy job runs
  `cargo test` and `pnpm test` first, and only on `refs/heads/main`.
- **The EU check.** Cloudflare's API documents `jurisdiction` (`"eu"`, `"fedramp"` or `"us"`)
  in each database of the list response, and wrangler's `d1 list --json` prints those objects
  as the API returns them
  (https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/list/).
  The deploy refuses a `clauseo-counts` whose `jurisdiction` is anything but `"eu"`, missing
  included.
- **Logs.** The Worker's own code writes no log line. workers-rs's panic hook (not behind a
  feature) and its JavaScript shim print to the runtime console on a panic. With observability
  off and no Tail Worker, that console is stored nowhere.
- **The deploy.** `counts-deploy.yml` runs on pushes to `main` that touch the Worker,
  `crates/counts` or `crates/protocol`, so a new value in the crate needs no Worker change. It
  finds the D1 database by name or creates it with `--jurisdiction=eu`, and never falls back to
  another jurisdiction. Its `check` action takes no input and prints only the two canary counts,
  as run logs on this public repo are public.

Left out of the launch cut, with no stubs: `/v1/crash` and R2, `/dash`, `/v1/query` and
`/v1/releases` with Access, JWT checks and `sqlparser`, the unreleased-version check and the
`release` table, `rollup`, `install_week` and the `v_*` views, the `AGENTS.md` query recipe, and
the `record-release.mjs` script that `release.yml` would run. `release.yml`'s "Record the
release (S23)" step stays as it is, skipped while `COUNTS_RELEASES_ENABLED` is unset.

## Consequences

- Reading the data needs D1's console or `wrangler d1 execute` with Rohan's own login until the
  dashboard and query endpoint land. No view hides small groups yet, so whoever queries sees
  install-level rows.
- **The rollup must land before the first `report` rows reach 90 days.** The cron deletes them
  then, and without `rollup` (and `install_week`) nothing of them survives. The first rows arrive
  when the app's endpoint is set (decision 0036, "The desktop").
- After launch:
  - `/v1/crash` and R2 with S22;
  - `/dash`, `/v1/query` and `/v1/releases` behind Access, with the JWT checks and the
    `sqlparser` guard;
  - the `release` table, the unreleased-version check, and `record-release.mjs` (then set
    `COUNTS_RELEASES_ENABLED`);
  - `rollup`, `install_week` and the `v_*` views with the under-5 rule;
  - the `AGENTS.md` recipe.
  Each later migration gets its own decision record.
- The caps are sized for the beta: about 3,000 installs' requests a day. Raising them is a change
  to `src/route.rs`, as production has no variable for them. A sender past the cap retries, so a
  full day delays counts but does not lose them while they stay within the 14-day window.
- A cron run's CPU is the same at any backlog: 33 statements with nothing returned. A range
  that holds many expired rows is D1's work, not the Worker's. If a run fails part-way, the ranges
  it finished stay deleted and the next day's run deletes the rest.
- The canary ids are public, so anyone can send rows under them. `check` showing rows means only
  that some request landed. That is why the live check runs it right after its own POST and
  deletes the rows.
- **Alerting / Workers Paid**, after launch. Nothing alerts on a spent cap, failing inserts or a
  filling database, and a flood can fill Free's 500 MB (above). Workers Paid raises the limits
  and Time Travel to 30 days; alerting should land with it or before.
- Moving to Workers Paid lengthens Time Travel to 30 days; `docs/counts-worker.md` then says so.
- `check-deploy.mjs` allows only the keys in `wrangler.toml` today. A new binding (R2, a var for
  Access) changes the checker in the same pull request.

## Status

proposed (Rohan confirms in review)

## Date

2026-11-11

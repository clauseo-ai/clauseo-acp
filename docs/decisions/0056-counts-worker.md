# 0056: The counts Worker, launch cut

## Context

Task S23 designs one Cloudflare Worker for Clauseo's telemetry. It takes the day reports and
weekly tallies of [decision 0036](0036-counts.md), the crash reports of S22 and the release job's
version rows. It also serves Rohan a dashboard behind Cloudflare Access and agents a read-only
SQL endpoint. For launch only the counts must arrive, and S22's crash reports are not built yet.
The orchestrator approved a launch cut: the two counts routes, D1, global hourly caps, the
retention cron, the privacy guards, the two workflows and the docs. The rest of S23 lands after
launch. The account runs on Workers Free for now, which allows 10 ms of CPU per invocation and
keeps D1 Time Travel for 7 days.

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
  capped inflate, the crate's parser, the hourly count, the insert. A refused request never
  touches D1.
- **Every refusal of the body is `400`, a body over the gzip cap too.** The app's sender
  (`crates/counts/src/sender.rs`, `answered`) treats `2xx` and `400` as final and retries anything
  else. A `413` for a body the app built too large would be retried with the same body until the
  report aged out.
- **Columns.** `report` and `tally` hold each closed top-level field of `DayReport` and
  `TallyReport`, in its wire form, plus `cells` as the crate re-serialises it. `schema` is not a
  column, as `0001_init.sql` holds schema 1 only. Enum columns hold the crate's wire names, so a
  new value needs no migration. Nothing records the time a row arrived, the sender's address or
  any header.
- **Global caps in D1, with no client key.** Each request that passes the parser adds one to its
  route's row for the hour, in a `rate` table (`INSERT … ON CONFLICT DO UPDATE … RETURNING n`).
  Over 250 an hour for counts or 100 for tallies gets `429`, and nothing is stored. The app retries
  later.
- **A custom domain.** `routes = [{ pattern = "t.clauseo.chat", custom_domain = true }]`.
  Cloudflare makes the DNS record and certificate when the Worker deploys, so the S23 route
  `t.clauseo.chat/*` and a DNS record made by hand are not needed.
- **Retention.** A pure-SQL cron on Mondays at 03:00 UTC deletes `report` after 90 days, `tally`
  after 396 and `rate` after 24 hours. The Worker computes the cut-offs with the crate's `Day` and
  `Week` and reads no rows, which fits Free's CPU limit. Time Travel adds 7 days on Workers Free,
  not S23's 30 on Paid.
- **The account ID.** The deploy job uses the repository variable `CLOUDFLARE_ACCOUNT_ID` when
  set. Otherwise it sets nothing, and wrangler uses the one account the token sees or fails with
  "More than one account available". No step runs `wrangler whoami` or prints the account.
- **The deploy.** `counts-deploy.yml` runs on pushes to `main` that touch the Worker,
  `crates/counts` or `crates/protocol`, so a new value in the crate needs no Worker change. It
  finds the D1 database by name or creates it with `--jurisdiction=eu`, and never falls back to
  another jurisdiction. Its `check` action prints one count for one given id, as run logs on this
  public repo are public.

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
- The caps are sized for the beta. Raising them is a change to `src/route.rs`, as production
  has no variable for them. A sender past the cap retries, so a full hour delays counts but does
  not lose them while they stay within the 14-day window.
- Moving to Workers Paid lengthens Time Travel to 30 days; `docs/counts-worker.md` then says so.
- `check-deploy.mjs` allows only the keys in `wrangler.toml` today. A new binding (R2, a var for
  Access) changes the checker in the same pull request.

## Status

proposed (Rohan confirms in review)

## Date

2026-11-11

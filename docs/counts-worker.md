# The counts Worker

`t.clauseo.chat` is a Cloudflare Worker that takes the anonymous usage counts the app sends
([decision 0036](decisions/0036-counts.md)) and keeps them in a D1 database. Its code is in
`services/counts-worker/`, beside the client that sends to it, so anyone can read exactly what it
accepts and stores. [Decision 0056](decisions/0056-counts-worker.md) records why it is built this
way and what waits until after launch.

## What it accepts

Two routes, nothing else:

| Request | Body |
| --- | --- |
| `POST /v1/counts` | 1 to 7 day reports, `{ "days": [...] }`, at most 16 KB gzip and 64 KB of JSON |
| `POST /v1/tally` | one weekly tally, at most 4 KB gzip and 16 KB of JSON |

The Worker checks a request in this order and stops at the first step that fails:

1. The method and path. Any other path gets `404`; another method on these two paths gets `405`.
2. The headers. `Content-Encoding: gzip` is required. A `Content-Length` over the gzip cap is
   refused.
3. The body, read up to the gzip cap. A body that passes the cap is refused at once, with or
   without a `Content-Length`.
4. The inflate, which stops one byte past the JSON cap. A body that would inflate further is
   refused.
5. The schema: `clauseo-counts`' own `parse_batch` or `parse_tally`, the functions and types the
   app writes with. An unknown field, an unknown value (an unknown `error_shown.code` too), a
   number past its cap, a day older than 14 days or not yet over, more than 7 days, a tally week
   older than 4 weeks or not yet over, or an `install` field in a tally is refused. "Today" and
   "this week" come from the Worker's clock.
6. The hourly cap ([below](#the-hourly-caps)).
7. The insert.

Steps 1 to 5 never touch the database. A refusal in steps 2 to 5 is `400` with an empty body, so
it does not say what was wrong. A stored request gets `204`. A day or tally sent again also gets
`204` and changes nothing.

The lists of allowed values are the crate's, built into the Worker when it deploys. A new value
in `crates/counts` (or a new error code in `crates/protocol`) reaches the Worker with the next
deploy, which runs on every push to `main` that touches either crate. Both workflows first run the
crate's schema snapshot test, so the Worker never deploys from types that differ from the
committed schema.

## What it stores

Three tables (`services/counts-worker/migrations/0001_init.sql`):

- `report`: one row per install and day, keyed `(install, day)`. Columns: `install`, `day`,
  `version`, `channel`, `os`, `os_major`, `arch`, `language`, and `cells`.
- `tally`: one row per tally, keyed by its random `tally` id. Columns: `tally`, `week`,
  `version`, `channel`, `os`, `os_major`, `arch`, `install_type`, `managed` (1 or 0), and
  `cells`. A tally has no install id, so it cannot be joined to a report.
- `rate`: one count per route per UTC hour, for the caps. It says nothing about who sent what.

Each column is a top-level field of the crate's `DayReport` or `TallyReport`, in its wire form.
`cells` is the cells as the crate writes them back: compact JSON in a fixed order. The Worker
stores the value it validated, never the bytes it received. A body with extra spaces or keys in
another order is stored exactly like a tidy one.

No table has a column for the time a row arrived. Nothing holds an IP address, a country, a
user agent or any other header. The source reads only `Content-Encoding` and `Content-Length`,
and a test fails if it reads any other header or names a client-address header. `report` and
`tally` are `WITHOUT ROWID` tables, so the order rows are stored in does not show the order they
arrived in.

## How long it keeps them

A cron runs every Monday at 03:00 UTC. It deletes:

- `report` rows whose day is more than 90 days old;
- `tally` rows whose week is more than 396 days old (about 13 months);
- `rate` rows more than 24 hours old.

Because the cron runs weekly, a row can last up to 7 days past its limit. D1's Time Travel can
also restore the database to any point in the last 7 days on Workers Free (30 days on Workers
Paid), and it cannot be turned off. So a deleted row can be recovered for that long, and a
restore to an earlier point can show roughly when rows arrived. There are no other backups or
exports.

## No logs

`wrangler.toml` turns Workers Logs, invocation logs and traces off. It has no workers.dev or
preview URL, no Tail Worker and no Logpush, and it sets no variable.
`services/counts-worker/scripts/check-deploy.mjs` fails the deploy if any of that changes. The
Worker writes no log line, and a test fails if its source names a logging macro.

Cloudflare still sees each request's IP address at its edge, as it does for any site it serves.
The Worker never reads it.

**Never run `wrangler tail` on the production Worker.** Tail streams each live request with its
headers and the sender's IP address.

## The hourly caps

The Worker stores at most 250 `/v1/counts` requests and 100 `/v1/tally` requests in each UTC hour,
across all senders. The caps are global, with no per-sender key, because there is nothing to key
them on. A request over a cap gets `429` and nothing is stored; the app keeps its report and
sends it again later. The caps are constants in `services/counts-worker/src/route.rs`. Production
has no setting that changes them. They are sized for the beta, and a later change raises them in
that file.

## Deploying

Two workflows, so no workflow that holds the Cloudflare token runs for a pull request:

- `.github/workflows/counts-worker.yml` runs on pull requests and pushes to `main` that touch
  the Worker, `crates/counts`, `crates/protocol` or the workflow. It runs the schema snapshot,
  then `cargo test` and clippy, the wasm build, and `pnpm test`. That last one runs the guards'
  tests and the Worker under `wrangler dev --local`, with a local D1 and the cron. It has no
  secrets.
- `.github/workflows/counts-deploy.yml` runs on the same pushes to `main`, and by hand. It is
  skipped until the repository variable `COUNTS_DEPLOY_ENABLED` is `true`. It runs in the
  environment `counts-deploy` (Rohan reviews), and only its Cloudflare steps get the secret
  `CLOUDFLARE_API_TOKEN`. Its steps:
  1. the schema snapshot test, then `check-deploy.mjs` on `wrangler.toml`;
  2. the account: the repository variable `CLOUDFLARE_ACCOUNT_ID` if set. If it is not set,
     wrangler uses the one account the token can see, and fails if it can see more than one;
  3. the D1 database `clauseo-counts`, found by name. The first deploy creates it with
     `--jurisdiction=eu`. If Cloudflare refuses that, the job fails; it never creates the
     database anywhere else;
  4. `wrangler.deploy.toml`, a copy of `wrangler.toml` with the database's real id, checked by
     `check-deploy.mjs --deployed`;
  5. `wrangler d1 migrations apply --remote`, then `wrangler deploy`. The deploy attaches
     `t.clauseo.chat` as a custom domain; Cloudflare makes its DNS record and certificate.

To check that one install's or tally's rows arrived, run the deploy workflow by hand with
`action: check` and the id in `install` or `tally`. It prints only how many rows hold that id (0
or 1). Run logs on this public repo are public, so the workflow never prints another row, a
total, an id or the account.

### One-time set-up

Before the first deploy, Rohan creates a Cloudflare API token with Workers and D1 edit rights for
the account and the `clauseo.chat` zone. He saves it as the `counts-deploy` environment's secret
`CLOUDFLARE_API_TOKEN`, and sets `COUNTS_DEPLOY_ENABLED` to `true`. The click-by-click steps
follow in [Cloudflare's guide to creating an API token](https://developers.cloudflare.com/fundamentals/api/get-started/create-token/)
and are added here when they are written. No secret is ever committed.

## Running it locally

From `services/counts-worker/`, with Rust (the repo's toolchain and the `wasm32-unknown-unknown`
target), `worker-build` (`cargo install --locked worker-build --version 0.8.7`), Node and pnpm:

```sh
cargo test --locked          # the request rules, rows and cut-offs, natively
pnpm install --frozen-lockfile
pnpm test                    # the guards, then the Worker under wrangler dev --local
```

`pnpm test` builds the Worker, gives each test file its own local D1 under `.wrangler/`, and
deletes it afterwards. It needs no Cloudflare account.

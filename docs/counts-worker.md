# The counts Worker

`t.clauseo.chat` is a Cloudflare Worker that takes the anonymous usage counts the app sends
([decision 0036](decisions/0036-counts.md)) and keeps them in a D1 database. Its code is in
`services/counts-worker/`, beside the client that sends to it, so anyone can read exactly what it
accepts and stores. [Decision 0056](decisions/0056-counts-worker.md) records why it is built this
way and what waits until after launch; [decision 0059](decisions/0059-counts-rollup-views.md)
adds the summaries and views.

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
   without a `Content-Length`. A missing or empty body is refused.
4. The inflate, which stops one byte past the JSON cap. A body that would inflate further is
   refused.
5. The schema: `clauseo-counts`' own `parse_batch` or `parse_tally`, the functions and types the
   app writes with. An unknown field, an unknown value (an unknown `error_shown.code` too), a
   number past its cap, a day older than 14 days or not yet over, more than 7 days, a tally week
   older than 4 weeks or not yet over, or an `install` field in a tally is refused. "Today" and
   "this week" come from the Worker's clock.
6. The daily cap ([below](#the-daily-caps)).
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

Five tables (`services/counts-worker/migrations/`), with no index beyond their keys:

- `report`: one row per install and day, keyed `(install, day)`. Columns: `install`, `day`,
  `version`, `channel`, `os`, `os_major`, `arch`, `language`, and `cells`.
- `tally`: one row per tally, keyed by its random `tally` id. Columns: `tally`, `week`,
  `version`, `channel`, `os`, `os_major`, `arch`, `install_type`, `managed` (1 or 0), and
  `cells`. A tally has no install id, so it cannot be joined to a report.
- `rate`: one count per route per UTC day, for the caps. It says nothing about who sent what.
- `install_week` (filled by the cron): one row per install per ISO week it reported in, keyed
  `(install, week)`. Columns: `install`, `week`, `first_week` (the install's cohort),
  `days_active`, `questions` (the week's `question_sent` counts), `hit_limit` (1 when a limit
  was reached that week) and `features` (how many distinct features were used).
- `rollup` (filled by the cron): monthly totals with no id, keyed `(month, version, os, event,
  cell)`. Columns: those, `agent` (the cell's own agent, if it has one), `count` (the cells'
  counts summed) and `installs` (distinct installs, counted while the reports still existed).
  A `_report` row counts each month's reports and installs per version and OS.

Each column is a top-level field of the crate's `DayReport` or `TallyReport`, in its wire form.
`cells` is the cells as the crate writes them back: compact JSON in a fixed order. The Worker
stores the value it validated, never the bytes it received. A body with extra spaces or keys in
another order is stored exactly like a tidy one.

No table has a column for the time a row arrived. Nothing holds an IP address, a country, a
user agent or any other header. The Worker's code reads only `Content-Encoding` and
`Content-Length`. (workers-rs copies the request's headers into memory when it wraps the request;
they are dropped with it.) A test fails if the code reaches any other header, the request's `cf`
metadata or the raw JS request, or names a client-address header. `report`, `tally`,
`install_week` and `rollup` are `WITHOUT ROWID` tables, so the order rows are stored in does not show the order they arrived in.

## How long it keeps them

A cron runs every day at 03:00 UTC. First it fills `install_week` and `rollup`
([below](#the-summaries)). Then it deletes:

- `report` rows whose day is more than 90 days old. A row goes at most 91 days after the day it
  counts;
- `tally` and `install_week` rows whose week is more than 396 days old. A row goes at most 403
  days after its week's Monday, which is 397 days after the week ends (about 13 months);
- `rate` rows of days before today.

`rollup` is kept indefinitely. It holds no install id, and each of its rows counts at least the
installs it says.

The deletes go by the first hex digit of the key, `0` to `f`: 16 ranges for `report`, 16 for
`tally`, one `install_week` delete and one `rate` delete. With the two fills that is 36
statements a run (Workers Free allows 50 D1 queries a run). No statement returns rows, so a run
costs the Worker the same CPU at any backlog, and one run clears it all. A statement that fails
does not stop the ones after it, so a failing fill never holds back a delete.

### The summaries

Both fills are SQL (`INSERT OR IGNORE … SELECT` with `json_each` over the cells,
`services/counts-worker/src/rollup.rs`); the Worker only works out two date ranges.

- A day report may arrive up to 14 days after its day, so a week or month is **final** once its
  last day is at least 15 days old.
- A run fills a period only if it is final and **whole**: none of its days is older than the 90
  days `report` keeps, so all of its rows still exist. A week can be filled from 15 to 90 days
  after its Monday and a month for at least 45 days, so a few failed runs lose nothing.
- Once filled, a period stays as it is: a later run inserts nothing. A period never filled
  while whole is never filled from part of its rows.

D1's Time Travel can also restore the database to any point in the last 7 days on Workers Free
(30 days on Workers Paid), and it cannot be turned off. So a deleted row can be recovered for
that long, and a restore to an earlier point can show roughly when rows arrived. There are no
other backups or exports.

## The views

The dashboard and `/v1/query` read only these views, never the tables
(`migrations/0002_rollup.sql`). Every view hides a group of fewer than 5: distinct installs, or
tallies for `v_tally`. No view has an `install` or `tally` column. An `event` that starts with
`_` is a total the view adds, not one of the app's events.

| View | From | One row per | Columns |
| --- | --- | --- | --- |
| `v_daily` | `report` (90 days) | day, version, OS, event and cell | `day`, `version`, `os`, `agent`, `event`, `cell`, `count`, `installs`; `_report` counts reports |
| `v_monthly` | `rollup` (kept) | month, version, OS, event and cell | as `v_daily`, with `month` |
| `v_active` | `report` (90 days) | day, ISO week or month | `period`, `start`, `installs` |
| `v_cohort_week` | `install_week` (13 months) | first week and week | `first_week`, `week`, `installs`, and `days_active`, `questions`, `hit_limit`, `features` summed |
| `v_paths` | `install_week` (13 months) | combination of buckets | `weeks_active` (1, 2, 4, 8, 16), `questions` in all (0, 1, 10, 100, 1000), `hit_limit` ever, `features` in the busiest week (0, 1, 2, 4), `installs` |
| `v_tally` | `tally` (13 months) | week, version, OS, event and cell | `week`, `version`, `os`, `event`, `cell`, `count`, `tallies`; `_tally` counts tallies, `_crashed` those with a crash, `_fleet` splits them by install type and managed |

A bucket is named by its lower bound: `questions = 10` means 10 to 99. `cell` is the cell's
fields without `n`, as compact JSON. The oldest and current periods of `v_active` hold only
part of their days. The rule hides each small group from direct view; subtracting one view from
another can still give the size of hidden groups, as decision 0059 says.

## No logs

`wrangler.toml` turns Workers Logs, invocation logs and traces off. It has no workers.dev or
preview URL, no Tail Worker and no Logpush, and it sets no variable.
`services/counts-worker/scripts/check-deploy.mjs` fails the deploy if any of that changes. The
Worker's own code writes no log line, and a test fails if its source names a logging macro. If
the Worker panics, workers-rs's panic hook and its JavaScript shim print an error to the runtime
console. With observability off and no Tail Worker, that console is stored nowhere.

Cloudflare still sees each request's IP address at its edge, as it does for any site it serves.
The Worker never reads it.

**Never run `wrangler tail` on the production Worker.** Tail streams each live request with its
headers and the sender's IP address.

## The daily caps

The Worker stores at most 3,000 `/v1/counts` requests and 1,000 `/v1/tally` requests in each UTC
day, across all senders. The caps are per day, not per hour, because the app sends its days within
minutes of UTC midnight. They are global, with no per-sender key, because there is nothing to key
them on. The count goes up only while it is under the cap
(`… DO UPDATE SET n = n + 1 WHERE n < cap`). A request past the cap gets `429`, writes nothing,
and leaves the count as it was. The app keeps its report and sends it again later. The caps are
constants in `services/counts-worker/src/route.rs`. Production has no setting that changes them.

What they mean on Workers Free, which allows 100,000 rows written and 5 million rows read a day,
and 500 MB per database:

- **Writes.** At both caps a day writes at most 3,000 × 8 rows (7 reports and the count) plus
  1,000 × 2 (the tally and the count), about 26,000. The cron's deletes also count as writes: at
  most about 22,000 a day once rows start expiring. That stays under 48,000.
- **Reads.** Each insert reads a handful of rows. The cron reads `report` three times a run (two
  fills and the deletes) and each other table about once: about 115,000 rows a day at 300
  installs. At the caps `report` would hold about 91 × 21,000 ≈ 1.9 million rows, and three
  scans of it (about 5.7 million) would pass Free's 5 million: a flood at the caps for 90 days
  would stop every query for the rest of that day, until 00:00 UTC.
- **Storage.** A realistic day report is about 1 KB, so 3,000 requests of 7 days add about
  21 MB a day. A deliberate flood of the largest valid bodies (64 KB each) could add about 192 MB
  a day and fill the 500 MB within days. Once the database is full, inserts fail until rows
  expire after 90 days or are deleted by hand. Nothing alerts on this yet; alerting and Workers
  Paid are after-launch items in decision 0056.

## Deploying

Two workflows, so no workflow that holds the Cloudflare token runs for a pull request:

- `.github/workflows/counts-worker.yml` runs on pull requests and pushes to `main` that touch
  the Worker, `crates/counts`, `crates/protocol` or the workflow. It runs the schema snapshot,
  then `cargo test` and clippy, the wasm build, and `pnpm test`. That last one runs the guards'
  tests and the Worker under `wrangler dev --local`, with a local D1, the caps and the cron. It
  has no secrets.
- `.github/workflows/counts-deploy.yml` runs on the same pushes to `main`, and by hand from
  `main`. It is skipped until the repository variable `COUNTS_DEPLOY_ENABLED` is `true`, and on
  any other ref. It runs in the environment `counts-deploy`. That environment's branch rule
  allows `main` only, and Rohan is its required reviewer. Its steps:
  1. with no secret: the schema snapshot test, `cargo test`, `pnpm test`, `check-deploy.mjs` on
     `wrangler.toml`, and the wasm build (`worker-build --release`);
  2. the D1 database `clauseo-counts`, found by name. The first deploy creates it with
     `--jurisdiction=eu`. If Cloudflare refuses that, the job fails; it never creates the
     database anywhere else. A database of that name whose `jurisdiction` is not `eu` fails the
     job too;
  3. `wrangler.deploy.toml`, a copy of `wrangler.toml` with the database's real id and without
     `[build]`, so wrangler uploads what step 1 built and builds nothing with the token present.
     `check-deploy.mjs --deployed` checks it;
  4. `wrangler d1 migrations apply --remote`, then `wrangler deploy`. The deploy attaches
     `t.clauseo.chat` as a custom domain; Cloudflare makes its DNS record and certificate.

Only the steps from 2 on get the secret `CLOUDFLARE_API_TOKEN`, and the optional secret
`CLOUDFLARE_ACCOUNT_ID`. Without the account ID, wrangler uses the one account the token can see,
and fails if it can see more than one. Run logs on this public repo are public. So every wrangler
call goes through `services/counts-worker/scripts/deploy.mjs`, which prints wrangler's output only
with UUIDs, 32-hex ids, emails and account names masked. It also masks the database id for the
rest of the job.

### The live check

The live check proves that a real install's counts arrive, without putting anyone's id in a
public log. It uses two fixed canary ids, `c0ffee00-0000-4000-8000-000000000001` for the install
and `c0ffee00-0000-4000-8000-000000000002` for the tally (`CANARY_INSTALL` and `CANARY_TALLY` in
`deploy.mjs`):

1. On a test machine, before counts are turned on, write the canary install id into
   `<data_dir>/counts/install_id`. The app reads an existing id from that file and keeps it
   (`read_install` in `crates/counts/src/store.rs`, used by `crates/counts/src/state.rs` when
   counts load), so its day reports arrive under the canary id.
2. The app picks a new random tally id each week, so send the canary tally by hand: the crate's
   example tally with `tally` set to the canary id, gzipped, to `POST /v1/tally`.
3. Run the deploy workflow by hand with `action: check`. It takes no input. It prints
   `canary report rows: N` and `canary tally rows: M`, then deletes the canary rows, so the check
   leaves nothing behind.

The canary ids are public, in this repo, so anyone can send rows under them. `check` showing
rows means only that *some* request with those ids landed, not that it was ours. That is why the
live check runs `check` right after its own POSTs, and deletes the rows each time.

### One-time set-up

Before the first deploy, Rohan creates the token and saves it; no secret is ever committed.

1. In the Cloudflare dashboard: My Profile, API Tokens, Create Token, Custom token. Name it
   `counts-deploy`, with three permissions: Account / D1 / Edit, Account / Workers Scripts / Edit,
   and Zone / Workers Routes / Edit. Account Resources: include his one account (not all
   accounts), so wrangler can find the account without an id. Zone Resources: include the
   specific zone `clauseo.chat`. No DNS or SSL right is needed: Cloudflare makes the custom
   domain's record and certificate itself. Without Zone / Workers Routes, every deploy after the
   first fails (workers-sdk issue 15863).
2. On GitHub: the repository's Settings, Environments, `counts-deploy`. Set its deployment
   branch rule to `main` only and Rohan as its required reviewer. Then Add environment secret,
   named `CLOUDFLARE_API_TOKEN`. Only if the token can see more than one account, add the
   account's ID as the environment secret `CLOUDFLARE_ACCOUNT_ID`.
3. The repository variable `COUNTS_DEPLOY_ENABLED` is set to `true`, and each deploy is approved
   in the `counts-deploy` environment.

See [Cloudflare's guide to creating an API token](https://developers.cloudflare.com/fundamentals/api/get-started/create-token/).

## Running it locally

From `services/counts-worker/`, with Rust (the repo's toolchain and the `wasm32-unknown-unknown`
target), `worker-build` on `PATH` (`cargo install --locked worker-build --version 0.8.7`), Node and
pnpm:

```sh
cargo test --locked          # the request rules, rows, the cron's plan and fill ranges
pnpm install --frozen-lockfile
pnpm test                    # the guards, then the Worker under wrangler dev --local
```

`pnpm test` builds the Worker, gives each test file its own local D1 under `.wrangler/`, and
deletes it afterwards. `tests/rollup.test.mjs` runs the cron's fills and
`tests/views.test.mjs` checks every view's under-5 rule and columns. It needs no Cloudflare account.

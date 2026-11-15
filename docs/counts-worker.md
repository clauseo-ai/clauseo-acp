# The counts Worker

`t.clauseo.chat` is a Cloudflare Worker that takes the anonymous usage counts the app sends
([decision 0036](decisions/0036-counts.md)) and keeps them in a D1 database. Its code is in
`services/counts-worker/`, beside the client that sends to it, so anyone can read exactly what it
accepts and stores. [Decision 0056](decisions/0056-counts-worker.md) records why it is built this
way and what waits until after launch, and [decision 0062](decisions/0062-counts-crash-route.md)
the crash route; [decision 0059](decisions/0059-counts-rollup-views.md) adds the summaries and
views, and [decision 0061](decisions/0061-counts-dash-query.md) the dashboard and the query route
behind Cloudflare Access.

## What it accepts

Three routes take counts and crash reports ([below](#reading-the-counts) for the two that read
them), nothing else:

| Request | Body |
| --- | --- |
| `POST /v1/counts` | 1 to 7 day reports, `{ "days": [...] }`, at most 16 KB gzip and 64 KB of JSON |
| `POST /v1/tally` | one weekly tally, at most 4 KB gzip and 16 KB of JSON |
| `POST /v1/crash` | one crash report, at most 2 KB gzip and 2 KB of JSON ([below](#crash-reports)) |

The Worker checks a request in this order and stops at the first step that fails:

1. The method and path. Any other path gets `404`; another method on these paths gets `405`.
2. The headers. `Content-Encoding: gzip` is required. A `Content-Length` over the gzip cap is
   refused.
3. The body, read up to the gzip cap. A body that passes the cap is refused at once, with or
   without a `Content-Length`. A missing or empty body is refused.
4. The inflate, which stops one byte past the JSON cap. A body that would inflate further is
   refused.
5. The schema: `clauseo-counts`' own `parse_batch`, `parse_tally` or `parse_crash`, the
   functions and types the app writes with. An unknown field, an unknown value (an unknown `error_shown.code` too), a
   number past its cap, a day older than 14 days or not yet over, more than 7 days, a tally week
   older than 4 weeks or not yet over, or an `install` field in a tally is refused. "Today" and
   "this week" come from the Worker's clock.
6. The daily cap ([below](#the-daily-caps)), or for a crash report the hourly one.
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

Ten tables (`services/counts-worker/migrations/`), with no index beyond their keys:

- `report`: one row per install and day, keyed `(install, day)`. Columns: `install`, `day`,
  `version`, `channel`, `os`, `os_major`, `arch`, `language`, and `cells`.
- `tally`: one row per tally, keyed by its random `tally` id. Columns: `tally`, `week`,
  `version`, `channel`, `os`, `os_major`, `arch`, `install_type`, `managed` (1 or 0), and
  `cells`. A tally has no install id, so it cannot be joined to a report.
- `rate`: one count per route per UTC day, for the caps. It says nothing about who sent what.
- `crash`: one row per crash report, keyed by its random `report` id. Columns: `report`,
  `week`, `version`, `os`, `arch`, `kind`, `area` and `signature` (a panic's `file:line`, a
  signal's `code:<number>`, a renderer's reason, or the kind alone). The report itself is in R2.
- `crash_rate`: the current UTC hour and how many crash reports it kept.
- `install_week` (filled by the cron): one row per install per ISO week it reported in, keyed
  `(install, week)`. Columns: `install`, `week`, `first_week` (the install's cohort),
  `days_active`, `questions` (the week's `question_sent` counts), `hit_limit` (1 when a limit
  was reached that week) and `features` (how many distinct features were used).
- `rollup` (filled by the cron): monthly totals with no id, keyed `(month, version, os, event,
  cell)`. Columns: those, `agent` (the cell's own agent, if it has one), `count` (the cells'
  counts summed) and `installs` (distinct installs, counted while the reports still existed).
  A `_report` row counts each month's reports and installs per version and OS.
- `day_cell` (filled by the cron): per day, version, OS, event and cell of the last 90 days, the
  summed `count` and the distinct `installs`, as `rollup` holds them per month. No id.
- `active` (filled by the cron): distinct installs per day, ISO week and month of the reports
  kept. No id.
- `cron_status` (written by the cron): per step (`delete`, `fill_week`, `fill_month`,
  `fill_day`, `fill_active`) and UTC day, how many runs finished the step (`ok`) and how many
  did not (`failed`). No id, and no time finer than the day.

Each column is a top-level field of the crate's `DayReport` or `TallyReport`, in its wire form.
`cells` is the cells as the crate writes them back: compact JSON in a fixed order. The Worker
stores the value it validated, never the bytes it received. A body with extra spaces or keys in
another order is stored exactly like a tidy one.

No table has a column for the time a row arrived. Nothing holds an IP address, a country, a
user agent or any other header. The Worker's code reads only `Content-Encoding`,
`Content-Length` and, on `/dash` and `/v1/query`, `Cf-Access-Jwt-Assertion`. (workers-rs
copies the request's headers into memory when it wraps the request; they are dropped with it.)
A test fails if the code reaches any other header, the request's `cf` metadata or the raw JS
request, or names a client-address header. `report`, `tally`, `install_week`, `rollup` and
`cron_status` are `WITHOUT ROWID` tables, so the order rows are stored in does not show the
order they arrived in.

### Crash reports

`POST /v1/crash` takes one crash report of S22's launch cut, as `crates/counts`' `CrashReport`
(decision 0060): no install or tally id, no time finer than the ISO week, no message, and no
path but a source file in one of two fixed forms. After the schema (a week older than 5 weeks or
in the future is refused):

1. Without the R2 bucket bound (the deploy leaves the binding out until the bucket exists), the
   Worker answers `503` and stores nothing. The app keeps the report and tries again.
2. The hourly cap: at most 200 reports in one UTC hour, across all senders, counted in
   `crash_rate` (one hour and its count; the hour's first report deletes the past hours). The
   count is the first statement; past the cap it is the only one, it changes nothing, and the
   answer is `429`.
3. The report, re-serialised by the crate (compact JSON in the crate's field order, never the
   request's bytes), goes to the R2 bucket `clauseo-crash`, in the EU jurisdiction, as
   `reports/<report id>.json`.
   The put is conditional (`onlyIf: { etagDoesNotMatch: "*" }`): it writes only when no object
   has the key.
4. One `crash` row ([below](#what-it-stores)), `INSERT OR IGNORE`. A report sent again answers
   `204` and leaves both the object and the row as first stored.

There is no edge rate-limit rule: `t.clauseo.chat` runs with WAF and rate-limiting rules off
(Rohan's decision), and the hourly cap is the only limit. A flood of valid reports can hold real
ones back for that hour, but none is lost: the app keeps each report and retries it, on `429`,
`503` or a network error, for up to 30 days. Forged reports can add a group to `v_crash`, which
only the owner and his agents read.

R2 keeps each object's exact upload time for the 90 days the object lives, which can show to
the second when a report arrived. S22's random delay of 1 to 24 hours before a report is sent
is the guard.

## How long it keeps them

A cron runs every day at 03:00 UTC. First it deletes:

- `report` rows whose day is more than 90 days old. A row goes at most 91 days after the day it
  counts;
- `tally` and `install_week` rows whose week is more than 396 days old. A row goes at most 403
  days after its week's Monday, which is 397 days after the week ends (about 13 months);
- `crash` rows whose week is more than 365 days old;
- `rate` rows of days before today, and `day_cell` and `cron_status` rows more than 90 days old.

Then it fills `install_week`, `rollup`, `day_cell` and `active` ([below](#the-summaries)),
and last it adds the run's outcome per step to `cron_status`.

The deletes come first so that the fills' scans of `report` can never use up D1's daily reads
before them; a quota spent by the fills would otherwise stop the deletes too, every day. The
order loses nothing: a report the deletes take is more than 90 days old, so every week or month
it belongs to started before the first day a fill still reads, and its last chance to be filled
was the day before.

`rollup` is kept indefinitely. An R2 lifecycle rule deletes each crash report's object 90 days
after its upload. It holds no install id, and each of its rows counts at least the
installs it says.

The deletes go by the first hex digit of the key, `0` to `f`: 16 ranges for `report`, 16 for
`tally`, and one each for `install_week`, `crash`, `rate`, `day_cell` and `cron_status`.
With the five fill statements and the health row that is 43 statements a run (Workers Free
allows 50 D1 queries a run). No statement returns rows, so a run costs the Worker the same CPU
at any backlog, and one run clears it all. A statement that fails does not stop the ones after
it, so a failing fill never holds back a delete. `/dash` shows the failures (below).

### The summaries

Every fill is SQL (`INSERT … SELECT` with `json_each` over the cells,
`services/counts-worker/src/rollup.rs`); the Worker only works out the date ranges.
`install_week` and `rollup` follow these rules:

- A day report may arrive up to 14 days after its day, so a week or month is **final** once its
  last day is at least 15 days old.
- A run fills a period only if it is final and **whole**: none of its days is older than the 90
  days `report` keeps, so all of its rows still exist. A week can be filled from 15 to 90 days
  after its Monday and a month for at least 45 days, so a few failed runs lose nothing.
- Once filled, a period stays as it is: a later run inserts nothing. A period never filled
  while whole is never filled from part of its rows.

`day_cell` and `active` exist so that `v_daily` and `v_active` need not scan `report` on every
query: D1 bills every row a query touches, the rows `json_each` makes from the cells and the
sort rows included, and over `report` those two views read about 325,000 and 190,000 rows a
query at 300 installs.

- `day_cell` is recomputed for the last 15 days (a report may still arrive for them) and for
  any day it has no row for. A group is written only when its numbers changed, so a quiet day
  writes nothing.
- `active` is emptied and recomputed from every report kept, about a hundred rows.
- Both show what the last run saw: yesterday's reports appear after 03:00 UTC.

D1's Time Travel can also restore the database to any point in the last 7 days on Workers Free
(30 days on Workers Paid), and it cannot be turned off. So a deleted row can be recovered for
that long, and a restore to an earlier point can show roughly when rows arrived. There are no
other backups or exports.

## The views

The dashboard's charts and `/v1/query` read only these views, never the tables
(`migrations/0002_rollup.sql`; `0003_summaries.sql`, which moves `v_daily` and `v_active` onto
the cron's summaries with the same columns; and `0004_crash.sql`). Every view hides a group of
fewer than 5: distinct installs, or tallies for `v_tally`, reports for `v_crash`. No view has an
`install`, `tally` or `report` column. An `event` that starts with `_` is a total the view
adds, not one of the app's events.

| View | From | One row per | Columns |
| --- | --- | --- | --- |
| `v_daily` | `day_cell` (90 days) | day, version, OS, event and cell | `day`, `version`, `os`, `agent`, `event`, `cell`, `count`, `installs`; `_report` counts reports |
| `v_monthly` | `rollup` (kept) | month, version, OS, event and cell | as `v_daily`, with `month` |
| `v_active` | `active` (90 days) | day, ISO week or month | `period`, `start`, `installs` |
| `v_cohort_week` | `install_week` (13 months) | first week and week | `first_week`, `week`, `installs`, and `days_active`, `questions`, `hit_limit`, `features` summed |
| `v_paths` | `install_week` (13 months) | combination of buckets | `weeks_active` (1, 2, 4, 8, 16), `questions` in all (0, 1, 10, 100, 1000), `hit_limit` ever, `features` in the busiest week (0, 1, 2, 4), `installs` |
| `v_tally` | `tally` (13 months) | week, version, OS, event and cell | `week`, `version`, `os`, `event`, `cell`, `count`, `tallies`; `_tally` counts tallies, `_crashed` those with a crash, `_fleet` splits them by install type and managed |
| `v_crash` | `crash` (1 year) | week, version, OS, arch, kind, area and signature | `week`, `version`, `os`, `arch`, `kind`, `area`, `signature`, `reports`; signature `_all` groups the same less the signature |

A bucket is named by its lower bound: `questions = 10` means 10 to 99. `cell` is the cell's
fields without `n`, as compact JSON. The oldest and current periods of `v_active` hold only
part of their days. The under-5 rule holds for each view on its own, not for a query that
combines views: subtracting one view from another (`v_monthly` minus `v_daily`'s days, or a
total row minus its visible groups) can still give the size of hidden groups, as decision 0059
says. In `v_crash` this is direct: a group's `_all` row minus its visible signature rows is the
total of its hidden signatures. That is accepted because only Rohan and his agents can read the
views at all, and no row names a report.

## Reading the counts

Two more routes read the views, never the tables, each behind its own Cloudflare Access
application. They are for Rohan and his agents only, no one else: `/v1/query` can combine
views, so the under-5 rule does not protect its answers the way it protects each view
([The views](#the-views)).

| Request | Who | Answer |
| --- | --- | --- |
| `GET /dash` | Rohan, signed in through Access with his email | one HTML page, six charts |
| `POST /v1/query` | an agent with the service token `clauseo-counts-agent` | the rows of one query, JSON |

**Fail closed.** Each route answers `404` to everything unless the Worker knows the Access team
(`ACCESS_TEAM`), the route's application AUD tag (`DASH_AUD`, `QUERY_AUD`) and the route's one
caller: Rohan's email (`DASH_EMAIL`) for `/dash`, the client id of the service token
`clauseo-counts-agent` (`QUERY_CLIENT_ID`) for `/v1/query`. So `/v1/query` is `404` until that
token exists. These reach the Worker at deploy as `wrangler deploy --var` values
([Access](#access)), never through `wrangler.toml`.

**The Access check.** Access refuses anyone without a login or token before the Worker runs. The
Worker checks Access's JWT again, so a misconfigured application cannot open a route: the
`Cf-Access-Jwt-Assertion` token must be RS256 (`none` and `HS*` are refused), be unexpired,
come from `https://<team>.cloudflareaccess.com`, carry that route's own AUD, name the route's
caller (for `/dash` an `email` equal to `DASH_EMAIL`, ignoring case; for `/v1/query` a
`common_name` equal to `QUERY_CLIENT_ID`, which a person's token never has), and be signed by a
key from `https://<team>.cloudflareaccess.com/cdn-cgi/access/certs`, checked with WebCrypto. The
Worker keeps the keys for an hour, and fetches them again sooner only for a token naming a key
it does not have, at most once a minute. Anything else is `403` with an empty body. So a policy
added to an application by hand still lets no one else through the Worker. The tests
serve their own key through `ACCESS_CERTS_URL_DEV`, which only `wrangler dev` sets. The Worker
takes it only as a loopback `http://` address, and `check-deploy.mjs` refuses a config that
names it.

**`/dash`** is server-rendered HTML with inline SVG: no script and nothing loaded from anywhere,
under `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'`, with
`Cache-Control: no-store`. Its six charts: daily, weekly and monthly active installs
(`v_active`); the last 12 weekly cohorts (`v_cohort_week`); questions per active day per agent,
answer time and the onboarding funnel (`v_daily`); crash-free weeks per version (`v_tally`).
Below them, the cron's health: every final week or month that still has reports but no
`install_week` or `rollup` rows (a fill that has not happened, and will be lost when those
reports pass 90 days), how many of the last 30 days have a recorded run, and every step that
failed. It runs the six statements in `services/counts-worker/src/dash.sql` as one batch: the
charts' four read one view each (`v_cohort_week` and `v_tally` only for the last 14 weeks), and
the health's two read `report`'s days, `install_week`'s weeks, `rollup`'s keys and
`cron_status`. At 300 installs, each reporting every day, one load reads about 154,000 D1 rows
(`tests/access.test.mjs` measures it through the D1 binding against a target of 200,000).

**`/v1/query`** takes `{ "sql": "<one query>" }` as plain JSON, at most 16 KB, and answers a
JSON array of rows. The query is parsed with `sqlparser` in SQLite's dialect and refused with
`400` unless it is one `SELECT` or `WITH` whose every table is a `v_*` view or a CTE in an
enclosing scope. A CTE may not take a base table's name or a `sqlite_` one, a name may not be
qualified (`main.report`, and `main.v_daily` too, so no `temp.` name can shadow a view), names
compare without case as SQLite's do, `FROM` takes no table-valued function, no CTE is recursive
(`WITH RECURSIVE`, or a CTE naming itself or a later one, which SQLite runs as recursive
anyway: an unbounded one kept D1 busy and blocked the counts writes), a query weighs at most
4 tables in all (a view counts one wherever it sits, a CTE reference counts every table its CTE
reads, so a chain of CTEs that joins each one to itself is counted in full, and a subquery in
`FROM` what it reads, at least one), and functions come
from a short list: `count`, `sum`, `total`, `avg`, `min`, `max`, `group_concat`, `date`, `strftime`,
`coalesce`, `round`, `json_extract`. The Worker runs it as
`SELECT * FROM (<query>) LIMIT 1000`, written from the parsed statement, and answers `504` after
10 seconds. A query D1 cannot run is `400` too. The 4-table weight is a coarse limit, not a
cost bound: a 4-way join can still be slow, and the Worker's 10-second answer does not cancel
the query in D1. D1's own per-query time limit is what bounds a query's cost.

### Querying

For an agent on Rohan's Mac. The service token's two halves live in the Keychain under the
service `clauseo-counts-query`, as `client-id` and `client-secret`. Read them inside the command,
at call time, so they never reach a file, a variable you print, or a log:

```sh
curl -sS https://t.clauseo.chat/v1/query \
  -H "CF-Access-Client-Id: $(security find-generic-password -s clauseo-counts-query -a client-id -w)" \
  -H "CF-Access-Client-Secret: $(security find-generic-password -s clauseo-counts-query -a client-secret -w)" \
  -H "Content-Type: application/json" \
  --data '{"sql": "SELECT period, start, installs FROM v_active ORDER BY 1, 2"}'
```

Never echo, log or store either half, and never put them in a script or a commit. Access turns the
pair into the JWT the Worker checks. To cut an agent off, revoke the token in Zero Trust.

### Checking Access live

After the one-time set-up, and before the deploy that turns Access on is approved, run on
Rohan's Mac:

```sh
node services/counts-worker/scripts/check-access.mjs
```

It prints only yes/no lines, never a token, an email, an id or a JWT, and exits 1 if any says
no:

- `/dash` and `/v1/query` without credentials, and `/v1/query` with a made-up service token,
  are stopped by Access: a redirect to the team's login, or Access's own `403` page (the
  Worker's answers have empty bodies). Run before the service token exists, the last line shows
  that an application with no policy refuses everyone.
- With the service token from the Keychain (service `clauseo-counts-query`, read at call time),
  `POST /v1/query` of `SELECT 1 AS one FROM v_active LIMIT 1` answers `200`, and the
  `common_name` in the `CF_Authorization` JWT Access sets equals the token's client id, with
  its `.access` suffix: that is what the Worker compares with `QUERY_CLIENT_ID`.
- By hand: open https://t.clauseo.chat/dash and sign in. A page means the email matches
  `DASH_EMAIL`; a `403` means it does not.

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
  1. in the job `bundle`, with no secret and no environment: the schema snapshot test,
     `cargo test`, `pnpm test`, `check-deploy.mjs` on `wrangler.toml`, and the wasm build
     (`worker-build --release`). Then one sha256 (`scripts/bundle-hash.mjs`) over what the
     deploy ships: `build/`, `wrangler.toml`, `migrations/`, the deploy scripts and the
     lockfile that pins wrangler. Those files are the job's artifact; the deploy job downloads
     them, checks the hash and builds nothing, so the bytes approved are the bytes hashed;
  2. the R2 bucket `clauseo-crash`, found or created in the EU jurisdiction, with a lifecycle
     rule that deletes objects after 90 days (other rules are kept). If the token lacks Workers
     R2 Storage Edit (the API answers `401` or `403`), the step says so in one line and goes on:
     the deploy copy then has no R2 binding and `/v1/crash` answers `503`. Then the D1 database
     `clauseo-counts`, found by name. The first deploy creates it with
     `--jurisdiction=eu`. If Cloudflare refuses that, the job fails; it never creates the
     database anywhere else. A database of that name whose `jurisdiction` is not `eu` fails the
     job too;
  3. `wrangler.deploy.toml`, a copy of `wrangler.toml` with the database's real id, without
     the R2 binding unless the bucket is there, and without `[build]`, so wrangler uploads what step 1 built and builds nothing with the token present.
     `check-deploy.mjs --deployed` checks it;
  4. `wrangler d1 migrations apply --remote`, then the Access step ([below](#access)), then
     `wrangler deploy` with the Access values as `--var`. The deploy attaches `t.clauseo.chat`
     as a custom domain; Cloudflare makes its DNS record and certificate.
     A successful deploy uploads its hash as the artifact `counts-worker-deployed` (kept 90
     days).

A push that would ship the same Worker skips the deploy, and so the approval. Most
`crates/protocol` changes don't touch what the Worker compiles: the job `bundle` finds the hash
of the last deploy on `main` (the newest run whose deploy job ran, if it succeeded) and, when
it equals this bundle's, ends green with the deploy job skipped and the line "same Worker as
deployed (<hash>), no deploy" in the run's summary. Any doubt deploys: no hash found, an expired
one, a failed last deploy or a build that is not byte for byte the same. The hash cannot see a
change on Cloudflare's side, such as the R2 bucket appearing (the deploy copy then gains its
binding), or a change to `ACCESS_TEAM` or `ACCESS_DASH_EMAIL`. To force a deploy, run the
workflow by hand from `main` with `deploy` (`gh workflow run counts-deploy.yml -f action=deploy`);
it deploys whatever the hash says.

Only the steps from 2 on get the secret `CLOUDFLARE_API_TOKEN`, and the optional secret
`CLOUDFLARE_ACCOUNT_ID`. Without the account ID, wrangler uses the one account the token can see,
and fails if it can see more than one. Run logs on this public repo are public. So every wrangler
call goes through `services/counts-worker/scripts/deploy.mjs`, which prints wrangler's output only
with UUIDs, 32- and 64-hex ids, emails and account names masked. It also masks the database id,
the Access team and the AUD tags for the rest of the job.

### Access

The deploy step (`node scripts/deploy.mjs deploy`, the Access part in
`services/counts-worker/scripts/access.mjs`) sets up Cloudflare Access through the API before it
deploys:

1. It finds, by name, or creates two self-hosted Access applications:
   - `clauseo-counts-dash` on `t.clauseo.chat/dash`, with one Allow policy for the email in the
     secret `ACCESS_DASH_EMAIL`;
   - `clauseo-counts-query` on `t.clauseo.chat/v1/query`, with one Service Auth policy for the
     service token named `clauseo-counts-agent`. Until that token exists, the application has no
     policy of ours, so it lets no one in.

   It keeps that one policy up to date (a new email updates it) and deletes every other policy
   on the two applications, one added by hand or a second of the same name included, so each
   application ends every deploy with exactly one policy, the one it made (or none). A
   reusable policy (one shared between applications) is not deleted from here: the job fails,
   naming the application, to be fixed by hand. An application of that name on another path,
   or two of one name, fails the job.
2. It reads both applications' AUD tags, and the client id (not the secret) of the service
   token `clauseo-counts-agent` through the service tokens API, and passes them, with the
   repository variable `ACCESS_TEAM` and `ACCESS_DASH_EMAIL`, as `wrangler deploy
   --var ACCESS_TEAM:… --var DASH_AUD:… --var DASH_EMAIL:… --var QUERY_AUD:…
   --var QUERY_CLIENT_ID:…`. Without the email or the token, that caller's variable is left out
   and its route stays `404`.
3. It prints none of them. They are masked like the database id, and errors name the API path
   without its ids.

It reads everything before it changes anything, and checks the shape of what it reads: lists
where it reads lists, each policy with a string `id`, `name` and `decision` and a boolean
`reusable`, and each application's `policies` a list of the same.

If `ACCESS_TEAM` is unset, or the API answers `401` or `403` because the token lacks the Access
permissions, or an answer has another shape, the step prints one line saying it skipped Access
and the deploy goes on. `/dash`
and `/v1/query` then stay `404`.

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
4. For `/v1/crash` (until then it answers `503`): on the R2 overview, complete R2's checkout
   (Cloudflare asks for a card even on the free tier), and add Account / Workers R2 Storage /
   Edit to the `counts-deploy` token. The next deploy creates the bucket in the EU with its
   90-day rule and binds it.
5. For `/dash` and `/v1/query` (until then both answer `404`):
   - In Zero Trust, choose the team name. Set it as the repository variable `ACCESS_TEAM` (not a
     secret), and his login email as the `counts-deploy` environment secret `ACCESS_DASH_EMAIL`.
   - Add two permissions to the `counts-deploy` token: Account / Access: Apps and Policies /
     Edit, and Account / Access: Service Tokens / Read.
   - In Zero Trust, Access, Service credentials, Service Tokens: create `clauseo-counts-agent`
     with a 1-year duration. Copy the Client ID and Client Secret once, into the Mac's Keychain:
     `security add-generic-password -s clauseo-counts-query -a client-id -w` and the same with
     `-a client-secret` (each prompts for the value). Set a reminder for the expiry.
   - Turn on strict service-token authentication if it is not on. The next deploy creates the
     two applications and their policies.

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

# 0061: The counts dashboard and query route

## Context

[Decision 0056](0056-counts-worker.md) left `/dash` and `/v1/query` of task S23 for after launch;
Rohan ruled on 2026-11-15 that they go live before it. They read the views of
[decision 0059](0059-counts-rollup-views.md). The Worker runs on Workers Free: 10 ms of CPU per
request and 5 million D1 rows read a day. S23 puts both routes behind Cloudflare Access, checks
Access's JWT again in the Worker, and guards queries with `sqlparser`.

## Decision

- **Routes.** `GET /dash` and `POST /v1/query`, each behind its own Access application. Each
  answers `404` to everything (any method) unless the Worker has `ACCESS_TEAM` and that route's
  AUD (`DASH_AUD`, `QUERY_AUD`): it fails closed. Wrong method: `405`. Bad or missing token:
  `403`. All with an empty body.
- **Values at deploy, not in `wrangler.toml`.** `scripts/access.mjs`, run by the deploy step,
  finds or creates the two applications by name (`clauseo-counts-dash` on
  `t.clauseo.chat/dash` with an Allow policy for `ACCESS_DASH_EMAIL`; `clauseo-counts-query` on
  `t.clauseo.chat/v1/query` with a Service Auth policy for the token `clauseo-counts-agent`, or
  no policy while it does not exist), reads their AUD tags and passes them with `ACCESS_TEAM` as
  `wrangler deploy --var`. They are masked in the log like the database id, and nothing prints
  an id, AUD, team or email. Without `ACCESS_TEAM`, or on a `401`/`403` from the API (the token
  lacks the Access scopes), it skips with one line and deploys anyway. It uses the API's
  per-application policies and leaves policies it did not make alone. This replaces S23's
  `wrangler.toml` vars: `check-deploy.mjs` keeps refusing any var there.
- **The JWT check** (`src/access.rs`). RS256 only: `none`, `HS*` and every other `alg` are
  refused before anything else. Then `exp` (and `nbf`, with 60 s of skew), `iss` =
  `https://<team>.cloudflareaccess.com`, `aud` containing the route's own AUD, and a `kid` from
  the team's `/cdn-cgi/access/certs`. Claims come before the signature, so a refused token costs
  no WebCrypto call. The keys are kept an hour in the isolate and refetched sooner only for an
  unknown `kid`, at most once a minute. The signature sits behind a `Verifier` trait: WebCrypto
  (`crypto.subtle.importKey` and `verify`, bound with wasm-bindgen in `src/webcrypto.rs`) in the
  Worker, a stand-in in the native tests. The team must be a plain DNS label.
- **The dev certificate URL.** `ACCESS_CERTS_URL_DEV` points the check at test keys under
  `wrangler dev`. The Worker takes it only as `http://127.0.0.1:<port>/…` or
  `http://localhost:<port>/…`, which the edge cannot reach, so even if it were set in production
  the check would fail closed. `check-deploy.mjs` refuses any config that names it, the deploy
  copy included, and the deploy passes only the three variables above.
- **Headers.** The source test's allowlist gains `Cf-Access-Jwt-Assertion` and nothing else; it
  is read in `glue.rs`'s one `header` function, on the read routes only.
- **The query guard** (`src/query.rs`). One statement, a `Statement::Query` whose bodies are
  selects, set operations or `VALUES` (`WITH … INSERT` is refused). Names resolve by scope,
  without case as in SQLite: every table must be one of the six views or a CTE of an enclosing
  query. A qualified name is refused, `main.v_daily` included: it would read nothing more than
  `v_daily`, but refusing every schema name means `temp.` can never shadow a view, and the rule
  stays one line. So `sqlite_master`, `sqlite_schema`, `temp.sqlite_master`, `main.report`,
  `"report"`, `[report]`, `` `report` `` and `REPORT` are all refused (`src/query/tests.rs`).
  A CTE may not take a base table's name, or a `sqlite_`, `_cf_` or `d1_` one. `FROM` takes tables,
  subqueries and joins, never a table-valued function. Functions: `count`, `sum`, `total`,
  `avg`, `min`, `max`, `group_concat`, `date`, `strftime`, `coalesce`, `round`,
  `json_extract`. The Worker runs `SELECT * FROM (<query>) LIMIT 1000`, written from the parsed
  statement (so a trailing `--` cannot swallow the wrapper), raced against a 10 s timer: `504`.
  A query D1 refuses is `400`. The body is plain JSON `{ "sql": … }`, at most 16 KB, so an agent
  can send it with curl.
- **Free-plan changes from S23.** 1,000 rows, not 10,000, as moving 10,000 rows through the
  Worker risks the 10 ms. `sqlparser` builds without its `recursive-protection` feature (its
  `stacker` dependency does not build for wasm32); the parser's own depth limit stays.
- **The dashboard** (`src/dash.rs`, `src/dash.sql`). Server-rendered HTML and inline SVG, no
  script, nothing loaded from elsewhere, under `default-src 'none'; style-src 'unsafe-inline'`,
  with `no-store`, `nosniff`, `no-referrer` and `DENY` framing. Six charts: daily, weekly and
  monthly active installs (`v_active`); the last 12 weekly cohorts (`v_cohort_week`); questions
  per active day per agent, answer time and the onboarding funnel (`v_daily`); crash-free weeks
  per version (`v_tally`, shown as "≥ n%" where the crashed tallies are under 5 and hidden). Its
  four chart statements run in one D1 batch with the health's two, sum in SQL, and return a few
  hundred rows. Each chart statement passes the query guard, so the charts read only views. The answer-time and onboarding orders come from the
  crate's `Took::ALL` and `OnboardingStep::ALL`.
- **The dashboard's cost, and two summaries.** D1's `rows_read` counts every row a query
  touches: table rows, each row `json_each` makes from the cells, and sort rows. Measured
  through the D1 binding with 300 installs reporting every day for 90 days (27,000 reports,
  17,100 install weeks and tallies), the page as first written read about 750,000 rows:
  `v_active` 189,000 and `v_daily` 325,000 on their own. So migration `0003_summaries.sql`
  adds two tables the cron fills, and redefines `v_daily` and `v_active` over them with the
  same columns and the same under-5 rule:
  - `day_cell` holds `v_daily`'s groups. Each run recomputes the last 15 days (a report may still
    arrive for them) and any day without rows, writing a group only when it changed; old days
    go with `report`. About 95,000 rows read a day, once 456,000 for the first backfill.
  - `active` holds `v_active`'s rows. Each run empties it and recomputes it from `report`:
    about 162,000 rows read and 200 written.
  - The chart statements on `v_cohort_week` and `v_tally` take only the last 98 days (in
    strftime's `%W` weeks, as the guard has no `printf`). SQLite pushes that bound into the view,
    so older rows are not expanded.
  The page now reads 153,651 rows at that load (`v_active` 107, `v_cohort_week` 66,096,
  `v_daily` 2,163, `v_tally` 41,899, the health 43,386), under the 200,000 target.
  `tests/access.test.mjs` seeds the same load, runs the cron, and checks the sum. The views now
  show what the last run (03:00 UTC) saw. The cron reads about 600,000 rows a day in all, and an
  agent's `v_daily` query a few thousand instead of 325,000.
- **A fill that keeps failing is shown** (review of #1066). The same migration
  adds `cron_status`: per step (`delete`, `fill_week`, `fill_month`, `fill_day`,
  `fill_active`) and UTC day, the runs
  that finished it and the runs that did not. It holds no id and no time finer than a day, and
  rows go after 90 days. Each run writes one row per step after its statements; a step counts as
  finished when all of its statements did. `/dash` shows the failed steps of the last 30 days,
  how many of those days have a run at all, and every week or month that is final, still has
  reports, and has no `install_week` or `rollup` rows: a fill that has not happened, which is
  lost once its reports pass 90 days. `/v1/query` cannot read `cron_status`.
- **The deletes cannot be starved** (review of #1066). The cron now deletes first and fills
  after, reversing 0059's order. Past D1's 5 million reads a day every query fails until
  00:00 UTC, so two fills that spent the quota would stop the deletes, and on the next day the
  same again. With the deletes first, they run on a fresh day's quota (the cron is at 03:00 UTC)
  before any fill scan. Nothing is lost: the deletes take reports older than 90 days, and a fill
  reads only periods whose first day is 90 days old or newer, so every period a deleted report
  belongs to had its last chance the day before. `src/retention/tests.rs` checks this over 1,100
  days. A guard that skips the fills near the quota was the alternative; the Worker cannot read
  the account's usage without another token, so the order is the only guard that needs nothing.
  The summaries above also cut what the dashboard and agents spend from the same quota.

## Consequences

- Each migration gets a decision record: this one covers `0003_summaries.sql`.

- An agent can read only what the views show. A query can still subtract one view from another
  (0059, "Consequences").
- At 1,000 rows a query must aggregate; an agent wanting more pages with `WHERE`.
- Rotating Access's key costs at most a minute of `403` per isolate.
- The deploy token needs two more permissions (Access: Apps and Policies Edit, Access: Service
  Tokens Read). Until Rohan adds them, deploys go on and both routes stay `404`.
- The 504 race is not tested live: a 10-second query under `wrangler dev` would hold the local
  D1 for the rest of the run.
- `/v1/releases` and its Access application are still to come.

## Status

proposed (Rohan confirms in review)

## Date

2026-11-15

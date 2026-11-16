# 0059: The counts summaries and views

## Context

[Decision 0056](0056-counts-worker.md) shipped the counts Worker with `report`, `tally` and
`rate`, and a daily cron that deletes `report` rows after 90 days. It left `install_week`,
`rollup` and the `v_*` views of task S23 for later, and warned that the rollup must land before
the first reports reach 90 days, since nothing of them survives otherwise. Rohan ruled on
2026-11-15 that these, and `/dash` and `/v1/query` after them, go live before launch. The Worker
runs on Workers Free: 10 ms of CPU and 50 D1 queries per invocation, 5 million rows read and
100,000 written a day. So all adding up happens in SQL, never in a Rust loop over rows. S23 says
each migration after the first gets a decision record; this one covers
`migrations/0002_rollup.sql`.

## Decision

**Two tables, filled by the daily cron before its deletes** (`src/rollup.rs`):

- `install_week` (`WITHOUT ROWID`, key `(install, week)`): one row per install per ISO week it
  reported in, with `first_week`, `days_active` (1 to 7), `questions` (the `question_sent` counts
  summed), `hit_limit` (1 when a `limit` cell of kind `reached` came that week) and `features`
  (how many distinct `feature_used` features). Research 37 had a "questions bucket" and a
  features bitmask; the table keeps the exact sum and a count, the views bucket them, and no list
  of the crate's features is copied into SQL. `first_week` is the earliest week the install
  already has a row for, else the earliest in the run's range, so a cohort survives as long as
  the install shows up at least once every 13 months.
- `rollup` (`WITHOUT ROWID`, key `(month, version, os, event, cell)`): per month, version, OS,
  event and cell (the cell's fields without `n`, as compact JSON), the summed `count` and the
  distinct `installs`, counted while the reports still exist. `agent` is the cell's own `agent`
  field, NULL where the event has none. A `_report` row per month, version and OS counts the
  reports (install-days) and their installs. `WITHOUT ROWID` here only saves the separate key
  index; the table holds no id.
- Both fills are one `INSERT OR IGNORE … SELECT` each, with `json_each` over `cells`. The Worker
  only computes two date ranges with the crate's `Day` and `Week` and decodes no row.

**The final-period rule.** A day report is accepted up to 14 days after its day, so a week or
month is *final* once its last day is at least 15 days before today. A run fills a period only
if it is final and *whole*: its first day is no older than the 90 days `report` keeps, so every
row of it still exists, whatever happened to earlier runs. A week can be filled from 15 to 90
days after its Monday, a month for at least 45 days, so missed runs lose nothing. Once a period
is filled, a later run inserts nothing: the rows can only shrink, and every key they could make
is there (`INSERT OR IGNORE`). A period never filled while whole is never filled from part of
its rows. `src/rollup/tests.rs` walks 900 days and checks that every week and month is filled
on some day and only while final and whole.

**Retention.** `install_week` goes with `tally` after 396 days (one statement, `week < cut`);
`rollup` is kept. The cron stays daily at 03:00 UTC, as 0056 decided, not S23's weekly. A run
sends 36 statements (2 fills, 34 deletes) under its 40-statement budget. A failing statement no
longer stops the ones after it, so a failing fill never holds back a delete.

**Views.** Each hides a group of fewer than 5 (distinct installs; tallies for `v_tally`), and
none has an `install` or `tally` column (`tests/views.test.mjs` reads `PRAGMA table_info`). An
`event` starting with `_` is a total the view adds, not one of the crate's events.

| View | Reads | Columns | For |
| --- | --- | --- | --- |
| `v_daily` | `report` | day, version, os, agent, event, cell, count, installs | answer time, questions per agent, the onboarding funnel; `_report` gives active installs |
| `v_monthly` | `rollup` | month, version, os, agent, event, cell, count, installs | the same past 90 days |
| `v_active` | `report` | period (`day`, `week`, `month`), start, installs | DAU, WAU and MAU over all versions |
| `v_cohort_week` | `install_week` | first_week, week, installs, days_active, questions, hit_limit, features | weekly cohorts |
| `v_paths` | `install_week` | weeks_active, questions, hit_limit, features, installs | how installs use the app across their weeks, each measure a bucket named by its lower bound |
| `v_tally` | `tally` | week, version, os, event, cell, count, tallies | crash-free weeks per version (`_tally`, `_crashed`), the fleet (`_fleet`: install type and managed) |

`v_monthly` and `v_active` are not in S23's list. Without `v_monthly` nothing of `rollup` is
reachable through `/v1/query`, which reads only `v_*` views; `v_active` gives the first chart
without the undercount of adding up `v_daily`'s per-version groups, each of which may be hidden.

**Free-plan cost at 300 installs** (`report` about 27,000 rows, `install_week` and `tally` about
17,000 each at 13 months):

- The cron: the two fills scan `report` once each and the deletes once more, plus `tally` and
  `install_week`: about 115,000 rows read a day. Writes: about 300 `install_week` rows a week
  and a few thousand `rollup` rows a month, on top of 0056's deletes.
- Each view reads its table once: the totals (`_report`, `_tally`, `_crashed`, `_fleet`) ride
  along as extra cells added with `json_set`, `v_active`'s three periods come from a
  three-element `json_each`, and `CROSS JOIN` keeps the table as the outer loop. So a query of
  `v_daily` or `v_active` reads about 27,000 rows, `v_cohort_week`, `v_paths` or `v_tally`
  about 17,000, and `v_monthly` only `rollup`. `/dash` (next pull request) picks its queries
  against these.

## Consequences

- The under-5 rule hides each small group from direct view. It does not stop subtraction
  between views: `v_active`'s day total less `v_daily`'s shown `_report` groups is the sum of
  the hidden ones, which can reveal how many installs (1 to 4) ran one version on one OS that
  day. No view shows an id or one install's cells, so what subtraction can reveal is a small
  count, not a person. Coarser groups or noise would be the next step if that matters.
- The oldest and the current week and month in `v_active` hold only part of their days.
- At the daily caps' worst case (decision 0056: `report` near 1.9 million rows after a 90-day
  flood), the cron's three scans of `report` would pass Free's 5 million reads a day. At 300
  installs they read about 81,000. An index on `report(day)` would cut that but adds a write to
  every insert; it waits until the load calls for it.
- An install that changes version within a month counts in each version's `rollup` rows.
- A new event or field in the crate needs no change here: events, cells and agents are read from
  the JSON. Only `install_week` names the crate's wire names `question_sent`, `limit` (kind
  `reached`) and `feature_used`; a native test pins them to the crate's types.
- The first fills happen on the first cron after deploy: every week and month that is then final
  and whole. Reports older than that are already gone.

## Status

proposed (Rohan confirms in review)

## Date

2026-11-15

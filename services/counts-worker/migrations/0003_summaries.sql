-- The cron's own health, and two summaries the dashboard and queries read instead of scanning
-- `report` (decision 0061).
--
-- On Workers Free D1 bills every row a query touches: table rows, the rows json_each makes from
-- the cells, and sort rows. Built straight over `report`, v_daily and v_active read about
-- 325,000 and 190,000 rows a query at 300 installs. The cron now fills `day_cell` and `active`
-- once a day, and the two views read those, a few thousand rows at most.
--
-- No table here has an install id. Groups under 5 are kept in the tables and hidden by the
-- views, as with `rollup`.

-- One row per cron step and UTC day: how many runs that day finished the step and how many
-- did not. No time finer than the day. The cron deletes rows older than 90 days.
CREATE TABLE cron_status (
  step TEXT NOT NULL,        -- delete, fill_week, fill_month, fill_day or fill_active
  day TEXT NOT NULL,         -- YYYY-MM-DD, UTC
  ok INTEGER NOT NULL,       -- runs whose statements of this step all succeeded
  failed INTEGER NOT NULL,   -- runs in which any of them failed
  PRIMARY KEY (step, day)
) WITHOUT ROWID;

-- v_daily's groups, per day, version, OS, event and cell: what v_daily used to compute from
-- `report` on every query. The cron recomputes the last 15 days (reports arrive up to 14 days
-- late) and any day it has no row for, and deletes days older than 90 with `report`.
CREATE TABLE day_cell (
  day TEXT NOT NULL,          -- YYYY-MM-DD, UTC
  version TEXT NOT NULL,
  os TEXT NOT NULL,
  agent TEXT,                 -- the cell's `agent`; NULL for an event without one
  event TEXT NOT NULL,        -- the cells' key, or `_report` for the day's reports
  cell TEXT NOT NULL,         -- the cell's fields without `n`, compact JSON; `{}` for `_report`
  count INTEGER NOT NULL,     -- the cells' `n`, summed; the reports for `_report`
  installs INTEGER NOT NULL,  -- distinct installs with that cell that day
  PRIMARY KEY (day, version, os, event, cell)
) WITHOUT ROWID;

-- v_active's rows: active installs over all versions per day, ISO week and month of the
-- reports kept. The cron recomputes the whole table each run.
CREATE TABLE active (
  period TEXT NOT NULL,       -- day, week or month
  start TEXT NOT NULL,        -- YYYY-MM-DD, YYYY-Www or YYYY-MM
  installs INTEGER NOT NULL,  -- distinct installs that reported in the period
  PRIMARY KEY (period, start)
) WITHOUT ROWID;

-- The same columns and the same under-5 rule as in 0002, now over the summaries. Both show
-- what the last cron run (03:00 UTC) saw.
DROP VIEW v_daily;
CREATE VIEW v_daily AS
SELECT day, version, os, agent, event, cell, count, installs
FROM day_cell
WHERE installs >= 5;

DROP VIEW v_active;
CREATE VIEW v_active AS
SELECT period, start, installs
FROM active
WHERE installs >= 5;

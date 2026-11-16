-- The summaries the daily cron fills, and the views the dashboard and queries read
-- (docs/counts-worker.md, decision 0059).
--
-- `install_week` and `rollup` are filled by `INSERT OR IGNORE … SELECT` over `report`
-- (src/rollup.rs), only for weeks and months that are final (their last day is at least 15 days
-- old, past the 14 days a day report may arrive late) and whole (no day of them is older than
-- the 90 days `report` keeps). A run that finds a period already filled inserts nothing.
-- `install_week` is deleted after 396 days with `tally`; `rollup` is kept, and holds no id.
--
-- Every view hides a group of fewer than 5: distinct installs for the views over `report`,
-- `install_week` and `rollup` (`rollup.installs` was counted while the reports still existed),
-- tallies for `v_tally`. No view has an `install` or `tally` column. A row whose `event` starts
-- with `_` is a total the view adds, not one of the crate's events.
--
-- Each view reads its table once: the totals ride along as extra cells added with json_set,
-- and CROSS JOIN keeps the table as the outer loop. D1 bills a day's rows read.

-- One install's one final ISO week. WITHOUT ROWID, like `report`, so storage order does not
-- show arrival order.
CREATE TABLE install_week (
  install TEXT NOT NULL,         -- the install id, as in `report`
  week TEXT NOT NULL,            -- YYYY-Www, the ISO week
  first_week TEXT NOT NULL,      -- the earliest week the install has a row for (its cohort)
  days_active INTEGER NOT NULL,  -- days reported that week, 1 to 7
  questions INTEGER NOT NULL,    -- the week's question_sent counts, summed
  hit_limit INTEGER NOT NULL,    -- 1 when a limit cell of kind `reached` came that week, else 0
  features INTEGER NOT NULL,     -- how many distinct features feature_used counted that week
  PRIMARY KEY (install, week)
) WITHOUT ROWID;

-- The monthly totals of the reports, kept after the reports are gone. No install id.
CREATE TABLE rollup (
  month TEXT NOT NULL,       -- YYYY-MM
  version TEXT NOT NULL,
  os TEXT NOT NULL,
  agent TEXT,                -- the cell's `agent`; NULL for an event without one
  event TEXT NOT NULL,       -- the cells' key, or `_report` for the month's reports
  cell TEXT NOT NULL,        -- the cell's fields without `n`, compact JSON; `{}` for `_report`
  count INTEGER NOT NULL,    -- the cells' `n`, summed; the reports for `_report`
  installs INTEGER NOT NULL, -- distinct installs with that cell in the month
  PRIMARY KEY (month, version, os, event, cell)
) WITHOUT ROWID;

-- Per day, version, OS, event and cell, from the last 90 days of reports: the summed count
-- and the distinct installs. `_report` counts the reports of each day, version and OS.
CREATE VIEW v_daily AS
SELECT day, version, os, json_extract(cell, '$.agent') AS agent, event, cell,
  SUM(n) AS count, COUNT(DISTINCT install) AS installs
FROM (
  SELECT r.day AS day, r.version AS version, r.os AS os, r.install AS install,
    e.key AS event, json_remove(c.value, '$.n') AS cell, json_extract(c.value, '$.n') AS n
  FROM report AS r
  CROSS JOIN json_each(json_set(r.cells, '$._report', json('[{"n":1}]'))) AS e
  CROSS JOIN json_each(e.value) AS c
)
GROUP BY day, version, os, event, cell
HAVING COUNT(DISTINCT install) >= 5;

-- `rollup` as it is, past the 90 days: the same columns as v_daily, by month.
CREATE VIEW v_monthly AS
SELECT month, version, os, agent, event, cell, count, installs
FROM rollup
WHERE installs >= 5;

-- Active installs over all versions per day, ISO week and month of the last 90 days of
-- reports. The oldest week and month, and the current ones, hold only part of their days.
CREATE VIEW v_active AS
SELECT p.value AS period,
  CASE p.value WHEN 'day' THEN r.day WHEN 'month' THEN substr(r.day, 1, 7)
    ELSE printf('%s-W%02d', strftime('%Y', date(r.day, (3 - (strftime('%w', r.day) + 6) % 7) || ' days')), (CAST(strftime('%j', date(r.day, (3 - (strftime('%w', r.day) + 6) % 7) || ' days')) AS INTEGER) - 1) / 7 + 1) END AS start,
  COUNT(DISTINCT r.install) AS installs
FROM report AS r CROSS JOIN json_each('["day","week","month"]') AS p
GROUP BY period, start
HAVING COUNT(DISTINCT r.install) >= 5;

-- Weekly cohorts: per first week and week, the installs active, their days, questions,
-- installs that reached a limit, and distinct features summed over the installs.
CREATE VIEW v_cohort_week AS
SELECT first_week, week, COUNT(*) AS installs, SUM(days_active) AS days_active,
  SUM(questions) AS questions, SUM(hit_limit) AS hit_limit, SUM(features) AS features
FROM install_week
GROUP BY first_week, week
HAVING COUNT(*) >= 5;

-- Installs grouped by what their retained weeks add up to. Each measure is a bucket, given by
-- its lower bound: weeks_active 1, 2, 4, 8 or 16 weeks; questions 0, 1, 10, 100 or 1000 in all;
-- hit_limit 1 if any week reached a limit; features 0, 1, 2 or 4 distinct in its busiest week.
CREATE VIEW v_paths AS
SELECT weeks_active, questions, hit_limit, features, COUNT(*) AS installs
FROM (
  SELECT
    CASE WHEN COUNT(*) >= 16 THEN 16 WHEN COUNT(*) >= 8 THEN 8 WHEN COUNT(*) >= 4 THEN 4
      WHEN COUNT(*) >= 2 THEN 2 ELSE 1 END AS weeks_active,
    CASE WHEN SUM(questions) >= 1000 THEN 1000 WHEN SUM(questions) >= 100 THEN 100
      WHEN SUM(questions) >= 10 THEN 10 WHEN SUM(questions) >= 1 THEN 1 ELSE 0 END AS questions,
    MAX(hit_limit) AS hit_limit,
    CASE WHEN MAX(features) >= 4 THEN 4 WHEN MAX(features) >= 2 THEN 2
      ELSE MAX(features) END AS features
  FROM install_week
  GROUP BY install
)
GROUP BY weeks_active, questions, hit_limit, features
HAVING COUNT(*) >= 5;

-- Per week, version, OS, event and cell of the tallies: the summed count and the tallies.
-- `_tally` counts all tallies, `_crashed` those with any crash cell, and `_fleet` splits them
-- by `install_type` and `managed` (its cell).
CREATE VIEW v_tally AS
SELECT week, version, os, event, cell, SUM(n) AS count, COUNT(DISTINCT tally) AS tallies
FROM (
  SELECT t.tally AS tally, t.week AS week, t.version AS version, t.os AS os,
    e.key AS event, json_remove(c.value, '$.n') AS cell, json_extract(c.value, '$.n') AS n
  FROM tally AS t
  CROSS JOIN json_each(json_set(t.cells,
    '$._tally', json('[{"n":1}]'),
    '$._crashed', CASE WHEN json_array_length(t.cells, '$.crash') > 0 THEN json('[{"n":1}]') END,
    '$._fleet', json_array(json_object(
      'install_type', t.install_type, 'managed', t.managed, 'n', 1)))) AS e
  CROSS JOIN json_each(e.value) AS c
)
GROUP BY week, version, os, event, cell
HAVING COUNT(DISTINCT tally) >= 5;

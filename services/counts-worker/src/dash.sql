-- The six reads behind /dash (src/dash.rs), run as one D1 batch. A statement ends with a
-- semicolon at the end of its last line, and no comment holds one.
--
-- The first four read only views and pass /v1/query's guard. They return at most a few hundred
-- rows. SQLite materializes a CTE that is used twice, and pushes a WHERE on a view's grouped
-- column down into it, so the 98-day bound (a little over the 12 weeks shown, in strftime's
-- %W weeks, which can be one off ISO's) keeps v_cohort_week and v_tally from expanding older
-- rows. v_active and v_daily read the cron's summaries (migration 0003).
SELECT period, start, installs FROM v_active
ORDER BY period, start;
WITH c AS (
  SELECT first_week, week, installs FROM v_cohort_week
  WHERE first_week >= strftime('%Y-W%W', 'now', '-98 days'))
SELECT first_week, week, installs FROM c
WHERE first_week IN (SELECT DISTINCT first_week FROM c ORDER BY first_week DESC LIMIT 12)
ORDER BY first_week, week;
SELECT event,
  CASE event WHEN 'answer_ended' THEN json_extract(cell, '$.took')
    WHEN 'onboarding_step' THEN json_extract(cell, '$.step') ELSE agent END AS bucket,
  CASE WHEN event IN ('_report', 'question_sent') THEN day END AS on_day,
  SUM(count) AS count, SUM(installs) AS installs
FROM v_daily
WHERE event IN ('_report', 'question_sent', 'answer_ended', 'onboarding_step')
GROUP BY 1, 2, 3
ORDER BY 1, 2, 3;
WITH t AS (
  SELECT week, version, event, SUM(count) AS count FROM v_tally
  WHERE week >= strftime('%Y-W%W', 'now', '-98 days') AND event IN ('_tally', '_crashed')
  GROUP BY week, version, event)
SELECT week, version, event, count FROM t
WHERE week IN (SELECT DISTINCT week FROM t ORDER BY week DESC LIMIT 12)
ORDER BY week, version, event;
-- The cron's health, from here on not views. Weeks and months that are final (their last day
-- 15 or more days back) and still have reports, but no install_week or rollup rows: a fill that
-- has not happened. One scan of report (its days), one of install_week, and key lookups in
-- rollup. Only period labels come back.
WITH days AS (SELECT DISTINCT day FROM report WHERE day <= date('now', '-15 days')),
weeks AS (
  SELECT DISTINCT printf('%s-W%02d', strftime('%Y', date(d.day, (3 - (strftime('%w', d.day) + 6) % 7) || ' days')), (CAST(strftime('%j', date(d.day, (3 - (strftime('%w', d.day) + 6) % 7) || ' days')) AS INTEGER) - 1) / 7 + 1) AS week
  FROM days AS d
  WHERE date(d.day, (6 - (strftime('%w', d.day) + 6) % 7) || ' days') <= date('now', '-15 days')),
months AS (
  SELECT DISTINCT substr(day, 1, 7) AS month FROM days
  WHERE date(day, 'start of month', '+1 month', '-1 day') <= date('now', '-15 days'))
SELECT 'week' AS period, week AS start FROM weeks
WHERE week NOT IN (SELECT week FROM install_week WHERE week >= (SELECT MIN(week) FROM weeks))
UNION ALL
SELECT 'month', month FROM months
WHERE NOT EXISTS (SELECT 1 FROM rollup WHERE rollup.month = months.month)
ORDER BY 1, 2;
-- The last 30 days of cron_status: per step and day, runs finished and failed.
SELECT step, day, ok, failed FROM cron_status
WHERE day >= date('now', '-30 days')
ORDER BY day, step;

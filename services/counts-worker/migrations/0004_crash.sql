-- The crash reports of S22's launch cut, as the counts Worker stores them (decision 0062).
--
-- Each report's re-serialised JSON goes to R2 (`clauseo-crash`, EU) as reports/<report id>.json;
-- this table holds its closed fields, so reports can be counted without reading R2. A report
-- holds no install id or tally id and no time finer than its ISO week. The daily cron deletes
-- rows whose week is more than 365 days old; an R2 lifecycle rule deletes objects after 90 days.

-- One crash report. A report sent again changes nothing. WITHOUT ROWID, so storage order does
-- not show arrival order.
CREATE TABLE crash (
  report TEXT NOT NULL,     -- the report's own random id, a lowercase UUID v4
  week TEXT NOT NULL,       -- YYYY-Www, the ISO week it was found in
  version TEXT NOT NULL,    -- the app's version
  os TEXT NOT NULL,         -- the crate's Os name
  arch TEXT NOT NULL,       -- the crate's Arch name
  kind TEXT NOT NULL,       -- panic, signal or renderer_gone
  area TEXT NOT NULL,       -- the crate's CrashArea name
  signature TEXT NOT NULL,  -- file:line, code:<number>, the renderer's reason, or the kind
  PRIMARY KEY (report)
) WITHOUT ROWID;

-- The global hourly cap: one count for the current UTC hour, with nothing about who sent what.
-- The Worker deletes past hours each time it counts a report.
CREATE TABLE crash_rate (
  hour TEXT NOT NULL,  -- YYYY-MM-DDTHH, UTC
  n INTEGER NOT NULL,  -- reports kept that hour
  PRIMARY KEY (hour)
) WITHOUT ROWID;

-- Reports per week, version, OS, arch, kind, area and signature; and per the same less the
-- signature, as signature `_all`, so a kind spread over many small signatures still shows.
-- Every group of fewer than 5 reports is hidden. No report column.
CREATE VIEW v_crash AS
SELECT week, version, os, arch, kind, area, signature, COUNT(*) AS reports
FROM crash
GROUP BY week, version, os, arch, kind, area, signature
HAVING COUNT(*) >= 5
UNION ALL
SELECT week, version, os, arch, kind, area, '_all', COUNT(*)
FROM crash
GROUP BY week, version, os, arch, kind, area
HAVING COUNT(*) >= 5;

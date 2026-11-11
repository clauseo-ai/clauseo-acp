-- Schema 1 of the usage counts, as the counts Worker stores it (docs/counts-worker.md).
--
-- Each table holds the validated value, re-serialised by clauseo-counts, never the request's
-- bytes. Every closed top-level field of the crate's DayReport and TallyReport is a column, in
-- its wire form (an enum's wire name, so a value the crate adds needs no migration); `cells` is
-- the cells as the crate writes them back (JSON, no whitespace).
-- `schema` is not a column: this migration holds schema 1 only, and a later schema gets its own
-- migration and decision record.
--
-- No table has a column for the time a row arrived, an address, a country or any header.
-- WITHOUT ROWID: rows are stored in key order, so storage order does not show arrival order.
-- The weekly cron deletes `report` rows after 90 days, `tally` rows after 396 and `rate` rows
-- after 24 hours.

-- One install's counts for one finished UTC day. A day sent again changes nothing.
CREATE TABLE report (
  install TEXT NOT NULL,   -- the install id, a lowercase UUID v4
  day TEXT NOT NULL,       -- YYYY-MM-DD, UTC
  version TEXT NOT NULL,   -- the app's version, X.Y.Z
  channel TEXT NOT NULL,   -- the crate's Channel name
  os TEXT NOT NULL,        -- the crate's Os name
  os_major INTEGER,        -- the OS's major version; NULL where the report leaves it out
  arch TEXT NOT NULL,      -- the crate's Arch name
  language TEXT NOT NULL,  -- the crate's Language name
  cells TEXT NOT NULL,     -- the day's non-zero cells, canonical JSON
  PRIMARY KEY (install, day)
) WITHOUT ROWID;

-- One install's weekly tally. It carries no install id, so it cannot be joined to `report`.
-- A tally sent again changes nothing.
CREATE TABLE tally (
  tally TEXT NOT NULL,         -- the tally's own random id, a lowercase UUID v4
  week TEXT NOT NULL,          -- YYYY-Www, the ISO week
  version TEXT NOT NULL,
  channel TEXT NOT NULL,
  os TEXT NOT NULL,
  os_major INTEGER,
  arch TEXT NOT NULL,
  install_type TEXT NOT NULL,  -- the crate's InstallType name
  managed INTEGER NOT NULL,    -- 1 when any policy source was read, else 0
  cells TEXT NOT NULL,         -- the week's crash and active_week cells, canonical JSON
  PRIMARY KEY (tally)
) WITHOUT ROWID;

-- The global hourly caps: one count per route per UTC hour, with nothing about who sent it.
CREATE TABLE rate (
  route TEXT,     -- counts or tally
  hour INTEGER,   -- hours since 1970-01-01 UTC
  n INTEGER,      -- requests that passed the schema in that hour
  PRIMARY KEY (route, hour)
) WITHOUT ROWID;

-- The cron's deletes read these.
CREATE INDEX report_day ON report (day);
CREATE INDEX tally_week ON tally (week);

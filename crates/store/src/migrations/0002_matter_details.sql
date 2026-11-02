-- Schema version 2: a matter's details for the rail and its place on the list.
-- docs/decisions/0022-matters-for-the-rail.md gives the reason for each column.
-- Frozen: a later change is a new numbered file in this folder, never an edit here.

ALTER TABLE matters ADD COLUMN client TEXT /* trimmed, 1 to 200 characters */;
ALTER TABLE matters ADD COLUMN next_date TEXT /* YYYY-MM-DD */;
ALTER TABLE matters ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE matters ADD COLUMN last_opened_ms INTEGER;
ALTER TABLE matters ADD COLUMN forgotten_ms INTEGER /* ForgetMatter's time; NULL while listed */;

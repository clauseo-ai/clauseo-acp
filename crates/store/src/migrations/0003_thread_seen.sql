-- Schema version 3: each thread's seen mark for the rail.
-- docs/decisions/0023-seen-marks-and-app-lists.md gives the reason for the column and the fill.
-- Frozen: a later change is a new numbered file in this folder, never an edit here.

ALTER TABLE threads ADD COLUMN seen_seq INTEGER /* MarkSeen: the thread's last seq then; NULL never */;
UPDATE threads SET seen_seq = (SELECT last_seq FROM streams WHERE stream = 'thread:' || threads.id);

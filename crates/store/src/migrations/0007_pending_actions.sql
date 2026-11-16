-- Schema version 7: the file action a matter's writer runs, from before it touches a file until
-- the writer passes on, so the next start can say which one a crash left unfinished.
-- docs/decisions/0006-store-format.md gives the reason for the table and its columns.
-- Frozen: a later change is a new numbered file in this folder, never an edit here.

CREATE TABLE pending_actions (matter TEXT PRIMARY KEY NOT NULL /* MatterId: one writer per matter */,
  kind TEXT NOT NULL /* InterruptedKind's JSON name */, files INTEGER /* files it set out to change; NULL unknown */,
  thread TEXT /* ThreadId it was asked from; NULL none */, at INTEGER NOT NULL /* ms since the epoch */) WITHOUT ROWID;

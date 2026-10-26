-- Schema version 1. docs/decisions/0006-store-format.md gives the reason for each choice.
-- Frozen: a later change is a new numbered file in this folder, never an edit here.

CREATE TABLE streams (stream_id INTEGER PRIMARY KEY /* below 2^31; rows never deleted, so never reused */,
  stream TEXT NOT NULL UNIQUE, last_seq INTEGER NOT NULL);
CREATE TABLE events (key INTEGER PRIMARY KEY /* (stream_id << 32) | seq */,
  event_id TEXT NOT NULL, schema_version INTEGER NOT NULL,
  kind TEXT NOT NULL /* R5's DomainEvent::kind(), the key of the envelope's event object, e.g. text_appended */,
  envelope BLOB NOT NULL /* the whole Envelope in MessagePack with named maps */);
CREATE VIEW events_readable AS /* read-only, for debugging in the sqlite3 CLI */
  SELECT s.stream AS stream, e.key & 4294967295 AS seq, e.event_id, e.schema_version, e.kind, e.envelope
  FROM events e JOIN streams s ON s.stream_id = e.key >> 32;
CREATE TABLE snapshots (stream_id INTEGER NOT NULL REFERENCES streams(stream_id), seq INTEGER NOT NULL,
  reducer_schema INTEGER NOT NULL,
  state BLOB NOT NULL /* R6's MessagePack, zstd level 3 */, PRIMARY KEY (stream_id, seq)) WITHOUT ROWID;
CREATE TABLE blobs (id INTEGER PRIMARY KEY, hash BLOB NOT NULL UNIQUE /* 32 bytes */,
  size_bytes INTEGER NOT NULL, data BLOB NOT NULL);
CREATE TABLE receipts (command_id TEXT PRIMARY KEY, stream_id INTEGER, payload_hash BLOB NOT NULL,
  result TEXT NOT NULL /* JSON */, ts_ms INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE raw_frames (id INTEGER PRIMARY KEY, stream_id INTEGER, dir INTEGER NOT NULL /* 0 in, 1 out */,
  ts_ms INTEGER NOT NULL, frame BLOB NOT NULL);
CREATE INDEX raw_frames_ts ON raw_frames(ts_ms);
CREATE TABLE matters (id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL /* scratch | folder */,
  folder_path TEXT NOT NULL, folder_locator BLOB /* OS file id or bookmark, written later */,
  shadow_repo_path TEXT NOT NULL, created_ms INTEGER NOT NULL);
CREATE TABLE threads (id TEXT PRIMARY KEY, matter_id TEXT NOT NULL REFERENCES matters(id),
  agent_id TEXT NOT NULL, title TEXT, acp_session_id TEXT, archived INTEGER NOT NULL DEFAULT 0,
  created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL);
CREATE INDEX threads_by_matter ON threads(matter_id, updated_ms DESC);
CREATE TABLE checkpoints (id TEXT PRIMARY KEY, matter_id TEXT NOT NULL REFERENCES matters(id),
  commit_oid TEXT NOT NULL, seq INTEGER NOT NULL /* seq of its CheckpointRecorded in the matter stream */);
CREATE INDEX checkpoints_by_matter ON checkpoints(matter_id, seq);
CREATE TABLE account_limits (agent TEXT NOT NULL, account TEXT NOT NULL, limits TEXT NOT NULL /* JSON */,
  PRIMARY KEY (agent, account)) WITHOUT ROWID;

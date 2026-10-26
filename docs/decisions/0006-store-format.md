# 0006: Store format

## Context

`store` keeps the append-only event log, snapshots, blobs, command receipts, the matter registry,
checkpoint links, the last account limits and, later, raw ACP frames. The host commits every
event there before it acts or publishes, and replays from there for subscribers and reopens.

The file outlives any one release. A later Clauseo must read what schema version 1 writes, so this
record gives the format exactly, with the reason for each choice. Saving each event and reopening a
long thread are hot paths, so most choices were measured. Every number below was measured on an
Apple M6.

The `store` row of the crates table in `ARCHITECTURE.md` listed only `protocol` and `rusqlite`
under "May depend on". The crate also needs `serde`, `serde_json`, `rmp-serde`, `blake3`, `zstd`,
`async-channel` and `futures`.

## Decision

### Files

- One database file, `state.sqlite`, in the folder the caller passes. The host joins it to the
  data folder; the store never resolves a data folder itself. SQLite adds `state.sqlite-wal` and
  `state.sqlite-shm` beside it.
- A lock file, `state.sqlite.lock`, beside it. `open` takes `File::try_lock` on it and holds the
  lock until `close`. A second open fails with `Error::Locked`. Reason: `seq` comes from one
  host's memory, so two processes on one file would collide. SQLite's own locks only take turns.
- On Unix, `open` creates `state.sqlite` and the lock file with mode `0o600` before SQLite opens
  the database. SQLite gives the `-wal` and `-shm` files the database's mode. Reason: the file
  holds privileged client work. Encryption at rest is the OS's full-disk encryption; there is no
  SQLCipher.
- `close` runs `PRAGMA wal_checkpoint(TRUNCATE)`, closes the connection, then releases the lock.

### Schema version 1

`crates/store/src/migrations/0001_init.sql`, verbatim (the test `decision_record_quotes_the_migration`
fails if this copy and the file differ):

```sql
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
```

In the table columns, ids are stored as lowercase hyphenated UUID text and times as milliseconds
since the Unix epoch. Inside `events.envelope`, the envelope's own encoding applies: ids are
16-byte MessagePack bin, as `docs/decisions/0002-event-schema.md` says, and `stream` is its text.

#### streams and the events key

- `streams.stream` is the text `thread:<uuid>` or `matter:<uuid>`, written and parsed with
  `StreamId`'s `Display` and `FromStr`. Every other table names a stream by its integer
  `stream_id`, which never leaves the store. `heads`, `read_after`, `reopen`, `Committed` and every
  envelope carry `StreamId`.
- The first commit, receipt or snapshot that names a stream gives it the next `stream_id`, as a row with
  `last_seq` 0. Rows are never deleted, so a number is never reused. The writer keeps the map from
  `StreamId` to `stream_id` and each `last_seq` in memory, loaded at `open`.
- An event's key is `(stream_id << 32) | seq`. Reason: a rowid table with that key stores no
  separate key index and no 43-byte stream text per row. Measured against a `(stream, seq)` key:
  file -30%, a 250-event transaction -50%, a range read -27%.
- So `seq` must be below 2^32 and `stream_id` from 1 to 2^31 - 1, which keeps the key positive.
  Anything else fails with `Error::KeyOutOfRange`. Both bounds are far off.
- `events_readable` is a read-only view that shows each row with its stream text and `seq`, for
  debugging in the `sqlite3` command line tool. No code reads it.
- `events.event_id` has no UNIQUE index. Reason: no query reads by `event_id`, and the host mints
  UUIDv7 ids. Measured: a 250-event transaction -27%, file -7%.
- `events.kind` is `DomainEvent::kind()`. `schema_version` is the envelope's own version.
- Rows of `events` are never updated or deleted. A per-stream `seq` runs from 1 without a gap; the
  store checks it and never assigns it.

#### The envelope

- `events.envelope` holds the whole `Envelope`, so decode and upcast have one input.
- It is MessagePack with named maps, written with `rmp_serde`'s named-map encoder, its default
  binary mode. Reason: measured against JSON text, the reopen tail decode was 41% faster (1,813 to
  1,847 µs down to 1,067 to 1,076 µs). Named maps key every field by name, so an older reader
  skips an unknown field and an unknown kind still decodes. The wire to the UI, `cli` and the
  WebSocket stays JSON: the store returns `Envelope` values, and their readers encode them.
- Envelopes are not compressed per row. Per-row zstd measured slower on every timing.
- Each row decodes in one pass, as `protocol`'s `EnvelopeRead`, straight from the row's borrowed
  bytes, with an `rmp_serde` deserializer built with `set_max_depth(MAX_MSGPACK_DEPTH)`. Nothing is
  copied first. Reason: buffering the payload first made the tail decode 34% slower.
- `EnvelopeRead::Newer` is `Error::NewerSchema` (`store.newer_version`). A row that does not
  decode is `store.corrupt`. A decoded envelope whose `stream` or `seq` differs from its row's key
  is `store.corrupt` too. So is a gap in `seq` on read, and a tail that stops short of the stream's
  `last_seq`.
- An older envelope is upcast on read by `protocol`, never rewritten.

#### Snapshots

- `snapshots.state` is the reducer's MessagePack (named maps), compressed with zstd level 3. The
  caller compresses with `SnapshotIn::compress` before the commit is queued, never the writer
  thread. Reason: compressing the 10 MB state of an 80-turn thread takes 16 to 20 ms, which on the
  writer would stall every other thread's commits.
- A commit that carries a snapshot deletes all but the two newest snapshots of its stream, so one
  bad snapshot has a fallback. A snapshot at the same `(stream_id, seq)` replaces the old one.
- A snapshot whose `seq` is past its stream's head, the commit's own events counted, fails the
  commit with `Error::SnapshotAhead`. Reason: kept, it would push out a real fallback, and once the
  log reached its `seq`, `reopen` would serve it as the fold of events it never saw.
- `reopen` reads the newest snapshot with the asked `reducer_schema`, at or below the head, that
  decompresses. One that does not decompress, or claims more than 1 GiB, is skipped, and the next
  older one, or the log from seq 1, takes its place. A snapshot with another `reducer_schema` is
  ignored. `reopen` decompresses on the reader thread and returns the MessagePack bytes, then the
  tail after the snapshot, in one read transaction. The store does not fold.
- When to snapshot is the host's call. The store exports `SNAPSHOT_EVERY_EVENTS` (2,000); the
  host also snapshots at each `TurnEnded`.

#### Blobs

- `blobs` is a rowid table, and its bytes are stored uncompressed. SQLite advises against
  `WITHOUT ROWID` for large rows. The host hashes each blob with BLAKE3 on its own thread
  (`BlobIn::new`) and puts the bytes in the same commit as the event that names them, so a blob and
  its event land in one transaction. Blobs insert with `OR IGNORE`, so the same bytes are stored
  once.
- `read_blob` finds the row id by `hash`, then reads the range with SQLite's incremental blob I/O,
  read-only, without loading the whole value. Measured, 16 KiB from a 1 MiB blob: 115.5 µs with
  `substr`, 3.6 µs with incremental blob I/O. Blobs still insert as a bound parameter, which
  measured faster than `zeroblob` plus a write.
- A read is cut at the blob's end and at `MAX_BLOB_READ_BYTES`. A read of the whole blob checks
  its hash. A missing blob, a size that differs from the row, or a hash that does not match is
  `store.corrupt`.

#### Receipts

- `receipts.result` is the command's `CommandOutcome` as JSON. `payload_hash` is the 32-byte
  BLAKE3 hash of the command's JSON (`command_hash`). A receipt names its stream by `stream_id`.
- A commit whose receipt names a `command_id` that already has one fails with
  `Error::DuplicateCommand`, carrying the stored receipt, so the host can tell a repeat from a
  conflict by `payload_hash`.
- A receipt is not part of the log and is not versioned by `schema_version`. One that does not
  decode counts as missing: `receipt` returns `None`, and a commit with the same `command_id`
  replaces it. That allows one re-run per `command_id`.

#### raw_frames

- Raw ACP frames are bounded and for debugging only: 7 days or 64 MiB, whichever is hit first
  (`RawFrameRetention`, in `StoreOptions`). `dir` is 0 for a frame in and 1 for one out.
- The table and the retention are part of schema 1. Capturing frames, in their own transaction
  that first prunes by age and then by size, is later work.

#### matters, threads, checkpoints

- `matters` is the matter registry. `kind` is `scratch` or `folder`; a kind this build does not
  know reads back as `MatterKind::Unknown`. `folder_locator` is written later. `matters` the crate
  never depends on `store`; the host writes the rows.
- `threads` holds each thread's summary row, listed per matter by `updated_ms`, newest first.
- A commit's matter and thread rows insert or replace the row with that id.
- `checkpoints` keeps only the id, the matter, the shadow repository's commit id and the `seq` of
  its `CheckpointRecorded` in the matter stream. The event already carries the rest; the commit id
  stays out of `protocol`, so it lives here.
- With `foreign_keys=ON`, a thread or checkpoint row must name a matter that has a row.

#### account_limits

- One row per agent and account holds the last `AccountLimits` as JSON. Limit ticks never enter
  the log. `agent` is the agent's serde name (`claude`, `codex`).
- The cache is disposable. A row that no longer decodes is skipped, and the host reads the limits
  again from their source.

#### No devices table

There is no `devices` table for phone pairing. Reason: an unused table freezes columns into the
format before any code uses them. A numbered migration adds it with the phone work.

### Pragmas

On the writer's connection, in this order:

1. `page_size=16384`, before `journal_mode` and before the first `CREATE`. The page size cannot
   change once the file exists; on an existing file this is a no-op. Measured against 4 KiB pages:
   file -18 to -22%, a 250-event transaction -39%, a range read -52%, a blob insert -61 to -80%.
   Cost: about 3 times more WAL bytes per commit (27.8 KiB at 4 KiB pages, 80.7 KiB at 16 KiB).
2. `journal_mode=WAL`. If SQLite does not report `wal`, `open` fails with `store.corrupt`.
3. `synchronous=FULL`. Reason: "committed before sent" must hold through a power cut, and the host
   acts on a commit once it returns. Measured, one 400-byte event per commit: NORMAL 19 µs, FULL
   39 µs, FULL with `fullfsync` 3,964 µs.
4. `temp_store=MEMORY`. Reason: group commit runs commits in savepoints, and with 16 KiB pages and
   the default (a temp file) the savepoints write a statement journal to disk. Measured: 2 grouped
   commits took 98 µs instead of 21 µs.
5. `foreign_keys=ON`. Turning it off was measured and rejected.
6. `fullfsync=ON`, on macOS only. On macOS, plain `fsync`, which FULL alone uses, does not flush
   the drive's write cache; only `F_FULLFSYNC` does. Group commit pays for it.
7. `busy_timeout=5000`, on both connections.
8. `journal_size_limit=33554432`: the WAL file left on disk after a checkpoint is cut to 32 MiB.
9. `max_page_count`, only when `StoreOptions.max_page_count` is set, so a test can force a full
   disk. Production passes `None`.

The reader's connection sets `query_only=ON` and `busy_timeout=5000`, and keeps SQLite's default
`cache_size` and no `mmap_size`. Memory-mapped I/O and a larger page cache were measured (reads 28
to 43% faster) and declined. With mmap, an I/O error on a mapped page arrives as a `SIGBUS` crash
instead of an SQLite I/O error, which bypasses the error mapping. A bigger page cache is private
memory that counts against the host's 40 MB idle memory budget. A non-default
`wal_autocheckpoint` was measured and rejected too.

### Durability and group commit

- Durable means the transaction holding the commit returned from its `COMMIT`. With
  `synchronous=FULL` (and `fullfsync` on macOS) the WAL is synced by then.
- One writer thread does every write. When it takes a commit request, it also takes every commit
  request already waiting in its queue, without waiting for more, and runs them in one
  transaction: each in its own `SAVEPOINT`, then one `COMMIT` and one sync for the group. A lone
  commit skips the savepoint. Any other request ends the group and is served after it.
- A commit that fails rolls back to its savepoint and gets its own error; the others go on. Each
  caller's reply goes out only after the shared `COMMIT` returns. A failed `COMMIT` fails every
  commit of the group with that error. If SQLite rolls back the whole transaction itself (it may
  on a full disk, an I/O error or busy), the commits released before it get the same error.
- The writer's in-memory heads and `stream_id` map change only for commits whose savepoint was
  released, once the `COMMIT` returns. Within a group, a commit's `seq` check counts the earlier
  released commits, so consecutive commits of one stream share a group. A commit after a failed
  one of the same stream fails with `SeqGap`, because the failed one's events were never written.
- Reason: a sync on macOS with `fullfsync` costs about 4 ms, so one sync per commit would cap the
  writer at about 250 commits a second. Measured with FULL and `fullfsync`: -44%, -73% and -90%
  per commit with 2, 4 and 8 commits waiting.
- Windows (`FlushFileBuffers`) and Linux (`fsync`) sync costs are unmeasured.

### Torn writes

SQLite's WAL frames carry checksums, and recovery drops a torn last transaction. So no per-record
checksum is added: the WAL already makes a torn write truncate cleanly, one level down.

### Errors

`Error::code()` gives the `ErrorCode` and `Error::retryable()` whether the same call may succeed
later. `retryable()` is exactly the code's retryable in `docs/decisions/0003-error-codes.md`, with
no exception, so it always equals the `retryable` of the `HostError` the host builds from the
code (`HostError::new` takes it from the code). A busy `store.io` is told apart by
`IoError::is_busy()`, for a host that wants to retry it at once. `Closed` is `internal`: after
`close` it is permanent, so a caller that trusted a retryable `Closed` would retry forever. The
code is in `crates/store/src/error.rs`.

| `Error` variant | When | code | retryable |
|---|---|---|---|
| `DiskFull` | `SQLITE_FULL` (also what `max_page_count` gives), or an OS write that failed with `ENOSPC` or `ERROR_DISK_FULL` | `store.disk_full` | no |
| `Corrupt { detail }` | `SQLITE_CORRUPT`, `SQLITE_NOTADB`, a gap in `seq` on read, an envelope that does not decode or names another row, a stored value that does not parse, a blob whose hash or size does not match | `store.corrupt` | no |
| `BlobMissing { hash }` | no blob has this hash | `store.corrupt` | no |
| `Io { source }` | every other SQLite or OS failure, `SQLITE_BUSY` after `busy_timeout` included (`is_busy()`) | `store.io` | yes |
| `Locked` | another process holds `state.sqlite.lock` | `store.locked` | no |
| `NewerSchema { found, supported }` | the file's `user_version`, or an envelope's `schema_version`, is newer than this build | `store.newer_version` | no |
| `SeqGap { stream, expected, got }` | a commit's events do not run on from the stream's last `seq` | `internal` | no |
| `DuplicateCommand { stored }` | the commit's receipt names a `command_id` that already has one | `internal` | no |
| `KeyOutOfRange { stream }` | a `seq` at or above 2^32, or a `stream_id` past 2^31 - 1 | `internal` | no |
| `Encode { detail }` | a value the caller passed does not serialise | `internal` | no |
| `SnapshotAhead { stream, seq, head }` | a commit's snapshot is past its stream's head, the commit's events counted | `internal` | no |
| `Constraint { detail }` | `SQLITE_CONSTRAINT`: a row breaks the schema, such as a thread or checkpoint row whose matter has no row | `internal` | no |
| `Closed` | a call after `close`, a call still queued when the store closed, or the store's thread is gone | `internal` | no |

The `internal` errors are host bugs, idempotency answers or a store already closed; the host
handles them before anything reaches a client.

The store never runs `PRAGMA integrity_check` on open, because its cost grows with the file.
Corruption is reported when a read meets it.

### Adding a migration

- A schema change is a new numbered file, `migrations/000N_<name>.sql` beside `0001_init.sql`,
  appended to `MIGRATIONS` in `crates/store/src/open.rs`, with `SCHEMA` raised to its number.
  Entry `i` of `MIGRATIONS` takes `user_version` from `i` to `i + 1`.
- `open` runs each migration past the file's `user_version` in one transaction that also sets
  `user_version`, so a migration lands whole or not at all.
- Never edit `0001_init.sql` or any merged migration. Never alter a table and ignore the error.
- An event shape change is an upcaster in `protocol`, not a rewrite of `events` rows.
- A file whose `user_version` is newer than `SCHEMA` is refused with `store.newer_version`.

### Interface decisions taken in this task

Rohan, 2026-10-26:

- `read_after(stream, after: Option<Seq>, limit)`: `None` reads from the first event. `limit` is
  capped at `READ_PAGE_MAX` (500).
- `Reopen.head` is `Option<Seq>`: `None` for a stream with no events.
- A commit's blobs are `BlobIn` values, hashed on the caller's thread by `BlobIn::new`, so the
  writer thread never hashes. `BlobIn::blob_ref()` gives the reference an event carries.

From the review of the first version (Rohan, 2026-10-26):

- When a store thread ends, by `Close`, by its queue closing or by a panic, it closes its queue and
  drops every request still in it, so each waiting caller gets `Error::Closed` instead of no answer.
- Dropping the last `Store` clone without `close` closes the store: the reader, then the writer,
  serves what is already queued, the writer checkpoints and closes the file, and the drop waits for
  both threads, so the lock is free when it returns. Only `close` reports how that went.
- `read_after` with `after` at or past 2^32 returns an empty page: no event can be there.
- `Closed` is `internal` and `retryable()` is the code's (Errors, above); `SQLITE_CONSTRAINT` is
  `Error::Constraint` and a snapshot past the head is `Error::SnapshotAhead`, both `internal`.

### Dependencies

The `store` row's "May depend on" cell in `ARCHITECTURE.md` becomes `protocol, rusqlite, serde,
serde_json, rmp-serde, blake3, zstd, async-channel, futures`. "Never depends on" stays `tauri`.
`rusqlite` turns on only `bundled` and `blob`; `blob` gives the incremental blob I/O for ranged
reads.

## Consequences

- Schema version 1 is a stored format. Every later build reads it, and every change to it is a new
  numbered migration with a record.
- The log is append-only and stores whole envelopes, so an old event is fixed by an upcaster in
  `protocol`, never by touching rows.
- Snapshots, receipts and the limits cache can stop decoding after a downgrade or a new variant;
  each falls back (the log, one re-run, a fresh read) instead of failing.
- A commit costs about 4 ms of sync on macOS. Group commit spreads it only when commits wait, so a
  caller that pipelines its commits with `send_commit` pays much less per commit than one that
  awaits each.
- One process per folder: a second Clauseo on the same data folder gets `store.locked`.

## Status

accepted

## Date

2026-10-26

//! The values a store call takes and returns: options, a commit and its parts, and the rows.

use std::path::PathBuf;

use clauseo_protocol::{
    AgentId, AgentSessionId, BlobHash, BlobRef, CheckpointId, CommandId, CommandOutcome, Count,
    Envelope, MatterId, MatterKind, Seq, StreamId, ThreadId, Timestamp,
};

use crate::blob::preview;

/// Where the store lives and how it behaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreOptions {
    /// The folder that holds `state.sqlite`, `state.sqlite.lock` and SQLite's `-wal` and `-shm`
    /// files. It must exist; the host joins it to the data folder. The store never resolves a
    /// data folder itself.
    pub dir: PathBuf,
    /// How long raw ACP frames are kept (the frames themselves are not captured yet).
    pub raw_frames: RawFrameRetention,
    /// SQLite's `max_page_count` for this connection, so a test can force `SQLITE_FULL`.
    /// Production passes `None`.
    pub max_page_count: Option<u32>,
}

impl StoreOptions {
    /// Options for `dir` with the default retention and no page cap.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            raw_frames: RawFrameRetention::default(),
            max_page_count: None,
        }
    }
}

/// The bound on the `raw_frames` table: frames older than `max_age_ms` go first, then the
/// oldest until the total is at most `max_bytes`. Decided with schema 1; the capture that
/// applies it is later work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawFrameRetention {
    /// The oldest frame kept, in milliseconds before the append's `now`: 7 days by default.
    pub max_age_ms: i64,
    /// The most frame bytes kept: 64 MiB by default.
    pub max_bytes: u64,
}

impl Default for RawFrameRetention {
    fn default() -> Self {
        Self {
            max_age_ms: 7 * 24 * 60 * 60 * 1000,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Everything one commit writes, all of it or none of it, in one savepoint.
///
/// Per stream, `events` must run on from the stream's last `seq` without a gap. `blobs` holds
/// the bytes of every blob an event names; `receipt` is the command's outcome; `thread` and
/// `matter` replace those rows; `checkpoints` are new rows; `snapshot` replaces all but the two
/// newest snapshots of its stream. A commit may carry a snapshot and no events.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Commit {
    /// The events, in order.
    pub events: Vec<Envelope>,
    /// The blobs the events name, each hashed by [`BlobIn::new`].
    pub blobs: Vec<BlobIn>,
    /// The receipt of the command that caused the events.
    pub receipt: Option<Receipt>,
    /// The thread's summary row, inserted or replaced.
    pub thread: Option<ThreadRow>,
    /// The matter's row, inserted or replaced.
    pub matter: Option<MatterRow>,
    /// New checkpoint rows.
    pub checkpoints: Vec<CheckpointRow>,
    /// A snapshot, already compressed by [`SnapshotIn::compress`].
    pub snapshot: Option<SnapshotIn>,
}

/// What a durable commit moved: the new last `seq` of each stream it wrote events to, in the
/// order the commit first named them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    /// `(stream, last seq)` per stream the commit's events touched.
    pub heads: Vec<(StreamId, Seq)>,
}

/// A blob's bytes and their BLAKE3 hash, computed once on the caller's thread, so the writer
/// thread never hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobIn {
    hash: BlobHash,
    bytes: Vec<u8>,
}

impl BlobIn {
    /// Hashes `bytes` with BLAKE3.
    pub fn new(bytes: Vec<u8>) -> Self {
        let hash = BlobHash::from_bytes(*blake3::hash(&bytes).as_bytes());
        Self { hash, bytes }
    }

    /// The hash.
    pub const fn hash(&self) -> BlobHash {
        self.hash
    }

    /// The bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The reference an event carries: the hash, the size and the preview that
    /// [`blob_ref`](crate::blob_ref) gives for the same bytes.
    pub fn blob_ref(&self) -> BlobRef {
        blob_ref_of(self.hash, &self.bytes)
    }
}

/// The reference for `bytes` whose hash is `hash`.
#[expect(
    clippy::expect_used,
    reason = "BlobRef::new checks only that the preview is at most PREVIEW_BYTES and no longer \
              than size_bytes; preview() returns a prefix of at most PREVIEW_BYTES, and a \
              buffer in memory is far below MAX_SAFE bytes, so it cannot fail"
)]
pub(crate) fn blob_ref_of(hash: BlobHash, bytes: &[u8]) -> BlobRef {
    let size = Count::clamped(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    BlobRef::new(hash, size, preview(bytes).to_owned())
        .expect("a prefix of at most PREVIEW_BYTES is a valid preview")
}

/// A command's stored outcome: the host's idempotency record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// The command, as the client named it.
    pub command_id: CommandId,
    /// The stream the command acted on, if any.
    pub stream: Option<StreamId>,
    /// [`command_hash`](crate::command_hash) of the command, to tell a repeat from a conflict.
    pub payload_hash: [u8; 32],
    /// What the command returned.
    pub result: CommandOutcome,
    /// When it was committed.
    pub ts: Timestamp,
}

/// A matter in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatterRow {
    /// The matter.
    pub id: MatterId,
    /// Its name.
    pub name: String,
    /// Scratch or folder.
    pub kind: MatterKind,
    /// The folder the lawyer works in.
    pub folder_path: String,
    /// An OS file id or bookmark for the folder; not written yet.
    pub folder_locator: Option<Vec<u8>>,
    /// Where its shadow repository lives.
    pub shadow_repo_path: String,
    /// When it was made.
    pub created: Timestamp,
}

/// A thread's summary row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRow {
    /// The thread.
    pub id: ThreadId,
    /// Its matter, which must have a row.
    pub matter: MatterId,
    /// Its agent.
    pub agent: AgentId,
    /// Its title, once it has one.
    pub title: Option<String>,
    /// The agent's session id, once attached.
    pub acp_session_id: Option<AgentSessionId>,
    /// Whether it is archived.
    pub archived: bool,
    /// When it was made.
    pub created: Timestamp,
    /// When it last changed.
    pub updated: Timestamp,
}

/// A checkpoint: the git commit behind a `CheckpointRecorded` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRow {
    /// The checkpoint.
    pub id: CheckpointId,
    /// Its matter, which must have a row.
    pub matter: MatterId,
    /// The shadow repository's commit id.
    pub commit_oid: String,
    /// The `seq` of its `CheckpointRecorded` in the matter's stream.
    pub seq: Seq,
}

/// A snapshot to store: the reducer's `MessagePack` bytes, zstd-compressed on the caller's
/// thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotIn {
    pub(crate) stream: StreamId,
    pub(crate) seq: Seq,
    pub(crate) reducer_schema: u32,
    pub(crate) compressed: Vec<u8>,
}

impl SnapshotIn {
    /// Compresses `state` (the reducer's encoded snapshot after the event at `seq`) with zstd
    /// level 3, on the calling thread. Call it off the host's actor, never on the writer.
    ///
    /// zstd fails only when it cannot allocate its context; then the snapshot holds no bytes,
    /// `reopen` skips it as one that does not decompress, and the host folds from the log.
    pub fn compress(stream: StreamId, seq: Seq, reducer_schema: u32, state: &[u8]) -> Self {
        let compressed = zstd::bulk::compress(state, SNAPSHOT_ZSTD_LEVEL).unwrap_or_default();
        Self {
            stream,
            seq,
            reducer_schema,
            compressed,
        }
    }

    /// The stream it belongs to.
    pub const fn stream(&self) -> StreamId {
        self.stream
    }

    /// The `seq` of the last event folded into it.
    pub const fn seq(&self) -> Seq {
        self.seq
    }

    /// The compressed bytes.
    pub fn compressed(&self) -> &[u8] {
        &self.compressed
    }
}

/// The zstd level of snapshots.
pub(crate) const SNAPSHOT_ZSTD_LEVEL: i32 = 3;

/// A stored snapshot as `reopen` returns it, decompressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The `seq` of the last event folded into it.
    pub seq: Seq,
    /// The reducer schema it was written with (the one `reopen` asked for).
    pub reducer_schema: u32,
    /// The reducer's `MessagePack` bytes.
    pub state: Vec<u8>,
}

/// What `reopen` loads: the newest snapshot with the asked reducer schema, and every event
/// after it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reopen {
    /// The snapshot, or `None` when there is none of that schema (fold `tail` from the start).
    pub snapshot: Option<Snapshot>,
    /// The events after the snapshot (all of them without one), in `seq` order.
    pub tail: Vec<Envelope>,
    /// The stream's last `seq`; `None` for a stream with no events.
    pub head: Option<Seq>,
}

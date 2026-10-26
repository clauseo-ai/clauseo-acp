//! `clauseo-store`: the append-only event log, snapshots, blobs, command receipts, the matter
//! registry, checkpoint links and the last account limits, in one SQLite file.
//!
//! # Owns
//!
//! `state.sqlite` in the folder the caller passes: schema version 1 and its numbered
//! migrations (`PRAGMA user_version`), the pragmas, the stored `MessagePack` envelope, the
//! exclusive lock (`state.sqlite.lock`), the private file mode, and the mapping of every disk
//! failure to `store.disk_full`, `store.corrupt` or `store.io`.
//! `docs/decisions/0006-store-format.md` records the format and the reason for each choice.
//!
//! # Interface
//!
//! [`Store::open`] with [`StoreOptions`] starts two named threads, `clauseo-store-writer` and
//! `clauseo-store-reader`. [`Store`] is `Clone`; every method is `async` and sends a request
//! over a bounded `async-channel`, with a `futures` oneshot for the reply. Write with
//! [`Store::commit`] or [`Store::send_commit`] (returns once queued; its [`CommitReply`]
//! resolves when durable) and a [`Commit`]: events, blobs ([`BlobIn`]), a [`Receipt`], a
//! [`ThreadRow`], a [`MatterRow`], [`CheckpointRow`]s and a [`SnapshotIn`]. Read with
//! [`Store::heads`], [`Store::read_after`], [`Store::reopen`] ([`Reopen`], [`Snapshot`]),
//! [`Store::receipt`], [`Store::read_blob`], [`Store::matters`], [`Store::threads`],
//! [`Store::checkpoint`] and [`Store::account_limits`]; [`Store::put_account_limits`] keeps
//! the last [`AccountLimits`](clauseo_protocol::AccountLimits) per account. [`blob_ref`] and
//! [`command_hash`] are the hashes the host puts in events and receipts. [`Error`] maps to
//! [`ErrorCode`](clauseo_protocol::ErrorCode) with [`Error::code`]. The `test-support`
//! feature exports `fixtures::reopen_20k`, the one generator of the 20k-event reopen fixture.
//!
//! # Invariants
//!
//! - The log is append-only: rows of `events` are never updated or deleted. An older envelope
//!   is upcast on read by `protocol`, never rewritten.
//! - Events are keyed by `(StreamId, Seq)`. The host assigns `seq`; a commit whose events do not
//!   run on from the stream's last `seq` fails with [`Error::SeqGap`]. Inside the store each
//!   stream has an integer `stream_id` (key `(stream_id << 32) | seq`); it never leaves the
//!   store.
//! - A commit is written whole or not at all. Every commit waiting in the writer's queue shares
//!   one transaction and one sync, each in its own savepoint, and no caller hears back before
//!   the shared `COMMIT` returns: then the commit is durable (`synchronous=FULL`, and
//!   `fullfsync` on macOS).
//! - Only the writer thread writes, and only one process opens the file at a time.
//! - Snapshots are compressed on the caller's thread ([`SnapshotIn::compress`]), never on the
//!   writer; blobs are hashed on the caller's thread ([`BlobIn::new`]).
//! - No clock reads: times come in as `Timestamp`. No `mmap`, no larger page cache, no
//!   `integrity_check` on open: corruption is reported when a read meets it.
//!
//! # Depends on
//!
//! `protocol`, `rusqlite` (bundled SQLite, `blob`), `serde`, `serde_json`, `rmp-serde`,
//! `blake3`, `zstd`, `async-channel`, `futures` and `thiserror`. Never `tauri`, `tokio`,
//! `reducer` or `matters`.
//!
//! # Start reading
//!
//! `store.rs` (the handle and the reader thread), `writer.rs` (the writer thread and group
//! commit) and `writer/apply.rs` (what one commit writes), `read.rs` (`read_after`, `reopen`
//! and the row reads), `open.rs` (lock, mode, pragmas, migrations) with
//! `migrations/0001_init.sql`, `codec.rs` (the key and the envelope), `blob.rs`, `error.rs`,
//! `model.rs` (the values calls take and return) and `write.rs` (the one place that creates
//! files). Tests are in `tests/store.rs` and its modules; the benches in `benches/`.

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod blob;
mod codec;
mod error;
#[cfg(feature = "test-support")]
pub mod fixtures;
mod model;
mod open;
mod read;
mod store;
mod write;
mod writer;

use clauseo_protocol::HostCommand;
use futures::channel::oneshot;

pub use blob::blob_ref;
pub use error::{Error, IoError};
pub use model::{
    BlobIn, CheckpointRow, Commit, Committed, MatterRow, RawFrameRetention, Receipt, Reopen,
    Snapshot, SnapshotIn, StoreOptions, ThreadRow,
};
pub use open::{DB_FILE, LOCK_FILE, SCHEMA};
pub use read::READ_PAGE_MAX;
pub use store::{CommitReply, Store};

/// How often the host snapshots a stream: every 2,000 events (and at each `TurnEnded`). When to
/// snapshot is the host's call; this is the rule's number.
pub const SNAPSHOT_EVERY_EVENTS: u64 = 2_000;

/// The BLAKE3 hash of `command`'s JSON, a receipt's `payload_hash`: equal hashes mean a repeat
/// of the same command, different ones a conflict on its `command_id`.
pub fn command_hash(command: &HostCommand) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    // A command that came in as JSON always writes back as JSON; were one ever to fail, the
    // hash would cover the bytes written before the failure, still the same for a repeat.
    if let Err(error) = serde_json::to_writer(&mut hasher, command) {
        hasher.update(error.to_string().as_bytes());
    }
    *hasher.finalize().as_bytes()
}

/// `PRAGMA <name>` on `conn`, its value as text.
#[cfg(feature = "test-support")]
fn pragma(conn: &rusqlite::Connection, name: &str) -> Result<String, Error> {
    use rusqlite::types::ValueRef;
    Ok(conn.query_row(&format!("PRAGMA {name}"), [], |row| {
        Ok(match row.get_ref(0)? {
            ValueRef::Null => String::new(),
            ValueRef::Integer(value) => value.to_string(),
            ValueRef::Real(value) => value.to_string(),
            ValueRef::Text(text) | ValueRef::Blob(text) => {
                String::from_utf8_lossy(text).into_owned()
            }
        })
    })?)
}

/// Sends a reply. A caller that stopped waiting dropped its receiver; nothing is left to tell.
pub(crate) fn answer<T>(reply: oneshot::Sender<T>, value: T) {
    drop(reply.send(value));
}

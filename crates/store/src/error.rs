//! `Error`: every way a store call fails, and the `ErrorCode` each one reaches a client as.
//!
//! The mapping is the one in `docs/decisions/0006-store-format.md`, Errors: a full disk is
//! `store.disk_full`, data that does not read back is `store.corrupt`, every other SQLite or OS
//! failure is `store.io`, and the host bugs (`SeqGap`, `DuplicateCommand`, `KeyOutOfRange`,
//! `Encode`) are `internal`.

use std::fmt;
use std::io;
use std::sync::Arc;

use clauseo_protocol::{BlobHash, ErrorCode, Seq, StreamId};
use rusqlite::ErrorCode as SqliteCode;

use crate::Receipt;

/// Why a store call failed. `Clone`, so one failed `COMMIT` can fail every commit of its group.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// The disk is full: SQLite's `SQLITE_FULL` (also what `max_page_count` gives), or an OS
    /// write that failed with `ENOSPC` or `ERROR_DISK_FULL`.
    #[error("the disk is full")]
    DiskFull,
    /// The file holds data that does not read back: `SQLITE_CORRUPT`, `SQLITE_NOTADB`, a gap
    /// in `seq` on read, an envelope that fails to decode or names another place than its row,
    /// a row value that does not parse, or a blob whose hash does not match its bytes.
    #[error("the store is corrupt: {detail}")]
    Corrupt {
        /// What did not read back, for logs.
        detail: String,
    },
    /// Any other SQLite or OS failure.
    #[error("store I/O failed: {source}")]
    Io {
        /// The failure.
        #[source]
        source: IoError,
    },
    /// Another process holds `state.sqlite.lock`: a second Clauseo on the same folder.
    #[error("another process holds the store")]
    Locked,
    /// The file, or an envelope in it, was written by a newer Clauseo: its `user_version` (or
    /// its envelope's `schema_version`) is `found`, and this build reads up to `supported`.
    #[error("written by a newer version: {found}, this build reads up to {supported}")]
    NewerSchema {
        /// The version found.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },
    /// A commit's events for `stream` do not run on from its last `seq`: a host bug.
    #[error("seq gap in {stream}: expected {expected}, got {got}")]
    SeqGap {
        /// The stream.
        stream: StreamId,
        /// The next `seq` the stream takes.
        expected: Seq,
        /// The `seq` the commit carried.
        got: Seq,
    },
    /// The commit's receipt names a `command_id` that already has one; `stored` is that
    /// receipt, so the host can compare `payload_hash` to tell a repeat from a conflict.
    #[error("command {} already has a receipt", stored.command_id)]
    DuplicateCommand {
        /// The receipt already stored.
        stored: Box<Receipt>,
    },
    /// A `seq` at or above 2^32, or a stream past the 2^31st, does not fit the `events` key.
    #[error("{stream} does not fit the events key")]
    KeyOutOfRange {
        /// The stream.
        stream: StreamId,
    },
    /// No blob has this hash. A reference that names a missing blob is a corrupt store.
    #[error("no blob {hash}")]
    BlobMissing {
        /// The hash looked up.
        hash: BlobHash,
    },
    /// A value the caller passed does not serialise: an envelope `protocol` refuses to write
    /// (an `Unknown` with a known kind) or a value with no JSON form. A host bug; nothing of the
    /// call was written.
    #[error("a value does not serialise: {detail}")]
    Encode {
        /// What failed, for logs.
        detail: String,
    },
    /// The store was closed, or its thread is gone.
    #[error("the store is closed")]
    Closed,
}

impl Error {
    /// The code a client sees.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::DiskFull => ErrorCode::StoreDiskFull,
            Self::Corrupt { .. } | Self::BlobMissing { .. } => ErrorCode::StoreCorrupt,
            Self::Io { .. } | Self::Closed => ErrorCode::StoreIo,
            Self::Locked => ErrorCode::StoreLocked,
            Self::NewerSchema { .. } => ErrorCode::StoreNewerVersion,
            Self::SeqGap { .. }
            | Self::DuplicateCommand { .. }
            | Self::KeyOutOfRange { .. }
            | Self::Encode { .. } => ErrorCode::Internal,
        }
    }

    /// Whether the same call may succeed later with nothing else changed. It follows
    /// [`ErrorCode::is_retryable`] (decision 0003), except that a `store.io` failure is
    /// retryable only for `SQLITE_BUSY` and [`Error::Closed`].
    pub fn retryable(&self) -> bool {
        match self {
            Self::Io { source } => source.busy,
            Self::Closed => true,
            Self::DiskFull
            | Self::Corrupt { .. }
            | Self::Locked
            | Self::NewerSchema { .. }
            | Self::SeqGap { .. }
            | Self::DuplicateCommand { .. }
            | Self::KeyOutOfRange { .. }
            | Self::BlobMissing { .. }
            | Self::Encode { .. } => self.code().is_retryable(),
        }
    }

    /// A `Corrupt` with `detail`.
    pub(crate) fn corrupt(detail: impl Into<String>) -> Self {
        Self::Corrupt {
            detail: detail.into(),
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(failure, _) = &error {
            let code = failure.code;
            if code == SqliteCode::DiskFull {
                return Self::DiskFull;
            }
            if code == SqliteCode::DatabaseCorrupt || code == SqliteCode::NotADatabase {
                return Self::corrupt(error.to_string());
            }
            let busy = code == SqliteCode::DatabaseBusy;
            return Self::Io {
                source: IoError::new(Inner::Sqlite(error), busy),
            };
        }
        // A stored value of the wrong type or range does not read back.
        if matches!(
            error,
            rusqlite::Error::FromSqlConversionFailure(..)
                | rusqlite::Error::InvalidColumnType(..)
                | rusqlite::Error::IntegralValueOutOfRange(..)
                | rusqlite::Error::Utf8Error(..)
        ) {
            return Self::corrupt(error.to_string());
        }
        Self::Io {
            source: IoError::new(Inner::Sqlite(error), false),
        }
    }
}

impl From<rusqlite::types::FromSqlError> for Error {
    /// A stored value of the wrong type does not read back.
    fn from(error: rusqlite::types::FromSqlError) -> Self {
        Self::corrupt(error.to_string())
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        // `StorageFull` is std's kind for ENOSPC and for Windows' ERROR_DISK_FULL.
        if error.kind() == io::ErrorKind::StorageFull {
            return Self::DiskFull;
        }
        Self::Io {
            source: IoError::new(Inner::Os(error), false),
        }
    }
}

/// The SQLite or OS failure behind [`Error::Io`]. Opaque, so the store's interface does not
/// name `rusqlite`'s types; its `Display` and `source` give the detail.
#[derive(Debug, Clone)]
pub struct IoError {
    inner: Arc<Inner>,
    busy: bool,
}

#[derive(Debug)]
enum Inner {
    Sqlite(rusqlite::Error),
    Os(io::Error),
}

impl IoError {
    fn new(inner: Inner, busy: bool) -> Self {
        Self {
            inner: Arc::new(inner),
            busy,
        }
    }

    /// Whether SQLite answered `SQLITE_BUSY`: another connection held the lock past
    /// `busy_timeout`.
    pub const fn is_busy(&self) -> bool {
        self.busy
    }
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.inner {
            Inner::Sqlite(error) => fmt::Display::fmt(error, f),
            Inner::Os(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl std::error::Error for IoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &*self.inner {
            Inner::Sqlite(error) => Some(error),
            Inner::Os(error) => Some(error),
        }
    }
}

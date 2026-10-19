//! Queries: reads a client makes without changing anything. They need no receipt and go through
//! the same shape validation as commands ([`HostQuery::validate`]).

use serde::{Deserialize, Serialize};

use crate::{
    AgentId, BlobHash, CheckpointId, CompactString, Error, FileChange, MAX_SAFE, MatterId,
    MatterKind, Seq, StreamId, ThreadId, ThreadStatus, ViewEntry,
};

/// Most entries one `PageEntries` returns: the size of a view snapshot (about 200 recent
/// entries), so a page is never larger than the `Reset` it continues from.
pub const MAX_PAGE_ENTRIES: u32 = 200;

/// Most bytes one `ReadBlob` returns (1 MiB): one reply stays bounded, and every M1 blob is text.
pub const MAX_BLOB_READ_BYTES: u32 = 1024 * 1024;

/// A read. JSON: `"list_matters"` or `{"list_threads": {...}}`; unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum HostQuery {
    /// Every matter.
    ListMatters,
    /// The threads of one matter.
    ListThreads {
        /// The matter.
        matter: MatterId,
    },
    /// Older entries of a stream's view, for scrolling back: up to `limit` entries with an
    /// `order` below `before_order`, newest last.
    PageEntries {
        /// The stream.
        stream: StreamId,
        /// Entries before this order; `None` pages from the newest entry.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        before_order: Option<Seq>,
        /// How many, 1 to [`MAX_PAGE_ENTRIES`].
        limit: u32,
    },
    /// The files that differ between two versions of a matter.
    CompareVersions {
        /// The matter.
        matter: MatterId,
        /// The older version.
        from: CheckpointId,
        /// The newer version.
        to: CheckpointId,
    },
    /// Part of a blob's text.
    ReadBlob {
        /// The blob.
        hash: BlobHash,
        /// The first byte to read, at most [`MAX_SAFE`].
        #[cfg_attr(feature = "specta", specta(type = u32))]
        #[serde(with = "crate::number::safe")]
        offset: u64,
        /// How many bytes, 1 to [`MAX_BLOB_READ_BYTES`].
        len: u32,
    },
}

impl HostQuery {
    /// Checks the query's shape: `PageEntries.limit` is 1 to [`MAX_PAGE_ENTRIES`];
    /// `ReadBlob.len` is 1 to [`MAX_BLOB_READ_BYTES`] and `offset` at most [`MAX_SAFE`].
    ///
    /// # Errors
    ///
    /// [`Error::OutOfRange`] naming the field's query, mapped to `command.invalid`.
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            Self::PageEntries { limit, .. } => in_range("PageEntries", *limit, MAX_PAGE_ENTRIES),
            Self::ReadBlob { offset, len, .. } => {
                if *offset > MAX_SAFE {
                    return Err(Error::OutOfRange {
                        kind: "ReadBlob",
                        value: i128::from(*offset),
                        min: 0,
                        max: i128::from(MAX_SAFE),
                    });
                }
                in_range("ReadBlob", *len, MAX_BLOB_READ_BYTES)
            }
            Self::ListMatters | Self::ListThreads { .. } | Self::CompareVersions { .. } => Ok(()),
        }
    }
}

fn in_range(kind: &'static str, value: u32, max: u32) -> Result<(), Error> {
    if (1..=max).contains(&value) {
        Ok(())
    } else {
        Err(Error::OutOfRange {
            kind,
            value: i128::from(value),
            min: 1,
            max: i128::from(max),
        })
    }
}

/// A query's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum QueryResult {
    /// For `ListMatters`.
    Matters(Vec<MatterSummary>),
    /// For `ListThreads`.
    Threads(Vec<ThreadSummary>),
    /// For `PageEntries`, newest last; fewer than `limit` means none older is left.
    Entries(Vec<ViewEntry>),
    /// For `CompareVersions`.
    FileChanges(Vec<FileChange>),
    /// For `ReadBlob`.
    Blob(BlobRange),
}

/// One matter in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MatterSummary {
    /// The matter.
    pub matter: MatterId,
    /// Its name.
    pub name: String,
    /// How it was made.
    pub kind: MatterKind,
}

/// One thread in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadSummary {
    /// The thread.
    pub thread: ThreadId,
    /// Its matter.
    pub matter: MatterId,
    /// Its title, once it has one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub title: Option<CompactString>,
    /// Its agent.
    pub agent: AgentId,
    /// Where it stands.
    pub status: ThreadStatus,
}

/// Part of a blob as UTF-8 text cut to whole characters.
///
/// `offset` is the byte where `text` starts: the requested one, moved forward to the next
/// character start if it fell inside a character. `end` is the byte just after the last returned
/// character, so the next read starts at `end`, and `end == size_bytes` means the blob is done. A
/// `len` too short for the character at `offset` still returns that one character, so `end`
/// always passes `offset` before the end. Both are at most [`MAX_SAFE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlobRange {
    /// The byte where `text` starts.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    #[serde(with = "crate::number::safe")]
    pub offset: u64,
    /// The byte just after `text`.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    #[serde(with = "crate::number::safe")]
    pub end: u64,
    /// The text.
    pub text: String,
}

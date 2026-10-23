//! Queries: reads a client makes without changing anything. They need no receipt, and every
//! field is checked by its type when the query decodes ([`PageLimit`], [`ReadLen`], [`Count`]),
//! so a query has no `validate` step; a refused query fails as `command.invalid`.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::{
    AgentId, BlobHash, CheckpointId, CompactString, Count, Error, FileChange, MatterId, MatterKind,
    Seq, StreamId, ThreadId, ThreadStatus, ViewEntry,
};

/// Most entries one `PageEntries` returns: the size of a `Reset`'s view window (about 200 recent
/// entries), so a page is never larger than the `Reset` it continues from.
pub const MAX_PAGE_ENTRIES: u32 = 200;

/// Most bytes one `ReadBlob` returns (1 MiB): one reply stays bounded, and every blob is text
/// today.
pub const MAX_BLOB_READ_BYTES: u32 = 1024 * 1024;

/// Defines a newtype over `u32` in `1..=$max`, which its constructor and its decoder both
/// check, so an out-of-range value never reaches the host.
macro_rules! bounded_u32 {
    ($(#[doc = $doc:expr])* pub struct $name:ident(1..=$max:ident);) => {
        $(#[doc = $doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "specta", derive(specta::Type), specta(type = u32))]
        pub struct $name(u32);

        impl $name {
            #[doc = concat!("Wraps `value`, which must be in 1..=[`", stringify!($max), "`].")]
            pub fn new(value: u32) -> Result<Self, Error> {
                if (1..=$max).contains(&value) {
                    Ok(Self(value))
                } else {
                    Err(Error::OutOfRange {
                        kind: stringify!($name),
                        value: i128::from(value),
                        min: 1,
                        max: i128::from($max),
                    })
                }
            }

            /// The number inside.
            pub const fn get(self) -> u32 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u32(self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

bounded_u32! {
    /// How many entries one `PageEntries` asks for: 1 to [`MAX_PAGE_ENTRIES`].
    pub struct PageLimit(1..=MAX_PAGE_ENTRIES);
}
bounded_u32! {
    /// How many bytes one `ReadBlob` asks for: 1 to [`MAX_BLOB_READ_BYTES`].
    pub struct ReadLen(1..=MAX_BLOB_READ_BYTES);
}

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
        /// How many.
        limit: PageLimit,
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
        /// The first byte to read, at most [`MAX_SAFE`](crate::MAX_SAFE).
        offset: Count,
        /// How many bytes.
        len: ReadLen,
    },
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
    /// Whether its queue is held, as [`HeaderEntry::queue_held`](crate::HeaderEntry::queue_held);
    /// missing reads as `false`.
    #[serde(default)]
    pub queue_held: bool,
}

/// Part of a blob as UTF-8 text cut to whole characters.
///
/// `offset` is the byte where `text` starts: the requested one, moved forward to the next
/// character start if it fell inside a character. `end` is the byte just after the last returned
/// character, so the next read starts at `end`, and `end == size_bytes` means the blob is done. A
/// `len` too short for the character at `offset` still returns that one character, so `end`
/// always passes `offset` before the end. Both are at most [`MAX_SAFE`](crate::MAX_SAFE).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlobRange {
    /// The byte where `text` starts.
    pub offset: Count,
    /// The byte just after `text`.
    pub end: Count,
    /// The text.
    pub text: String,
}

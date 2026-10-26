//! The stat cache in memory: what the last checkpoint saw of each file, and the rule that says
//! when a file must be read again. `history.rs` stores it as a git index file.

use std::fs::Metadata;
use std::time::{Duration, SystemTime};

use clauseo_protocol::MatterPath;

use crate::history::Oid;

/// A file's stat as git's index stores it: 32-bit fields, so a value read back from the cache
/// equals one taken from fresh metadata. Seconds before 1970 count as 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Stat {
    /// Modification time: seconds and nanoseconds since the Unix epoch.
    pub(crate) mtime: FileTime,
    /// Status change time on Unix, creation time on Windows.
    pub(crate) ctime: FileTime,
    /// Device id (Unix), truncated to 32 bits; 0 on Windows.
    pub(crate) dev: u32,
    /// Inode number (Unix), truncated to 32 bits; 0 on Windows.
    pub(crate) ino: u32,
    /// Size in bytes, truncated to 32 bits.
    pub(crate) size: u32,
}

/// Seconds and nanoseconds since the Unix epoch, as git's index stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct FileTime {
    pub(crate) secs: u32,
    pub(crate) nanos: u32,
}

/// How close to the cache's write time an mtime must be for the file to count as racily clean.
const RACY_WINDOW: Duration = Duration::from_secs(2);

impl FileTime {
    /// The time `time` names, clamped to the 32-bit seconds range.
    pub(crate) fn from_system(time: SystemTime) -> Self {
        let since = time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            secs: u32::try_from(since.as_secs()).unwrap_or(u32::MAX),
            nanos: since.subsec_nanos(),
        }
    }

    fn as_duration(self) -> Duration {
        Duration::new(u64::from(self.secs), self.nanos)
    }
}

impl Stat {
    /// The stat of `metadata`, taken without following a symlink.
    pub(crate) fn from_metadata(metadata: &Metadata) -> Self {
        let mtime = metadata
            .modified()
            .map(FileTime::from_system)
            .unwrap_or_default();
        let size = truncate(metadata.len());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let ctime = FileTime {
                secs: u32::try_from(metadata.ctime()).unwrap_or(0),
                nanos: u32::try_from(metadata.ctime_nsec()).unwrap_or(0),
            };
            Self {
                mtime,
                ctime,
                dev: truncate(metadata.dev()),
                ino: truncate(metadata.ino()),
                size,
            }
        }
        #[cfg(not(unix))]
        {
            let ctime = metadata
                .created()
                .map(FileTime::from_system)
                .unwrap_or_default();
            Self {
                mtime,
                ctime,
                dev: 0,
                ino: 0,
                size,
            }
        }
    }
}

/// The low 32 bits, as git's index keeps sizes, devices and inodes.
fn truncate(value: u64) -> u32 {
    u32::try_from(value & u64::from(u32::MAX)).unwrap_or(u32::MAX)
}

/// What the cache knows of one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheEntry {
    /// The file, relative to the matter folder.
    pub(crate) path: MatterPath,
    /// Its stat when the last checkpoint saw it.
    pub(crate) stat: Stat,
    /// Whether and how it is in the last checkpoint's tree.
    pub(crate) state: EntryState,
}

/// Whether a cached path is in the last checkpoint's tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryState {
    /// Read and hashed; in the tree as this blob.
    Read(Oid),
    /// In the tree as this earlier blob; the file is cloud-only now and was not read. Never
    /// clean: each walk checks again whether it is still cloud-only.
    Unread(Oid),
    /// Not in the tree: skipped, over the cap or cloud-only and never versioned.
    Skipped,
}

impl EntryState {
    /// The blob in the tree, if any.
    pub(crate) const fn blob(self) -> Option<Oid> {
        match self {
            Self::Read(oid) | Self::Unread(oid) => Some(oid),
            Self::Skipped => None,
        }
    }
}

/// The whole cache: one entry per path, sorted by path bytes (git's index order).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StatCache {
    /// The entries, sorted by path bytes.
    pub(crate) entries: Vec<CacheEntry>,
    /// The cache file's modification time when it was last written or loaded; `None` before
    /// the first write. Entries modified within 2 s of it are racily clean.
    pub(crate) written_at: Option<FileTime>,
}

impl StatCache {
    /// The entry for `path`, by binary search.
    pub(crate) fn get(&self, path: &str) -> Option<&CacheEntry> {
        find_by_path(&self.entries, path, |entry| entry.path.as_str())
    }

    /// The blob to reuse for `path` without reading it, if its stat is unchanged, it was read,
    /// and it is not racily clean.
    pub(crate) fn clean_blob(&self, path: &str, stat: &Stat) -> Option<Oid> {
        let entry = self.get(path)?;
        match entry.state {
            EntryState::Read(oid) if entry.stat == *stat && !self.is_racy(&entry.stat) => Some(oid),
            EntryState::Read(_) | EntryState::Unread(_) | EntryState::Skipped => None,
        }
    }

    /// Whether an entry's mtime is within [`RACY_WINDOW`] of the cache's write time (or after
    /// it), so a same-size rewrite in the same second could hide behind an equal stat.
    pub(crate) fn is_racy(&self, stat: &Stat) -> bool {
        match self.written_at {
            None => true,
            Some(written) => stat.mtime.as_duration() + RACY_WINDOW > written.as_duration(),
        }
    }
}

/// The item of `items`, sorted by path bytes (git's index order, which every list of files in
/// this crate keeps), whose path is `path`, by binary search. `key` gives an item's path.
pub(crate) fn find_by_path<'a, T>(
    items: &'a [T],
    path: &str,
    key: impl Fn(&T) -> &str,
) -> Option<&'a T> {
    items
        .binary_search_by(|item| key(item).as_bytes().cmp(path.as_bytes()))
        .ok()
        .and_then(|index| items.get(index))
}

#[cfg(test)]
mod tests;

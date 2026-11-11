//! The skip record: what a checkpoint's walk found in the folder but did not version, so an
//! undo knows, from the history alone, which paths were there at the start of a turn. The stat
//! cache knows it too, but it is a cache (rebuilt without skipped files, or lost); the record is
//! part of the commit.
//!
//! Every checkpoint commit names its record in a `Clauseo-Skips` trailer (`trailers.rs`): `none`
//! when the walk skipped no file and read every folder, otherwise the id of a blob holding it.
//! The blob is stored like any object but no tree names it; Clauseo never prunes objects, and
//! older readers ignore the trailer. A commit without the trailer (written before it) has no
//! record: what it skipped is unknown. The blob is text, one line per entry, sorted by path
//! bytes:
//!
//! ```text
//! clauseo-skips 1
//! skipped cloud-only 18 1700000000.000000000 notes.txt
//! stale 2048 1700000100.000000000 Lease.docx
//! unread Sub
//! ```
//!
//! - `skipped <reason> <size> <mtime seconds>.<nanoseconds> <path>`: a file not versioned, why
//!   (as `Clauseo-Baseline` spells it), and its size and modification time as the stat cache
//!   keeps them (32-bit size, as git's index).
//! - `stale <size> <mtime seconds>.<nanoseconds> <path>`: a file versioned earlier that is
//!   cloud-only now and was not read, so its content now is not known (a colleague may have
//!   changed it in the cloud); the tree keeps its earlier blob.
//! - `unread <folder>`: a folder that did not list (not downloaded, permission denied): nothing
//!   under it is known.
//!
//! The path is last and taken to the end of the line (a path holds no control character).
//! Readers skip a line they do not know.

use std::collections::BTreeMap;

use clauseo_protocol::{MatterPath, SkippedFileReason};

use crate::history::HistoryStore;
use crate::stat::{CacheEntry, EntryState, FileTime, Stat, find_by_path};

/// What a commit says of its skip record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SkipsRef {
    /// No `Clauseo-Skips`: written before records were, so nothing is known.
    #[default]
    Unrecorded,
    /// `none`: nothing skipped, every folder read.
    Empty,
    /// The blob holding the record.
    Blob(crate::history::Oid),
}

/// The first line of a record blob.
const HEADER: &str = "clauseo-skips 1";

/// One skipped file of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkipEntry {
    pub(crate) path: MatterPath,
    pub(crate) reason: SkippedFileReason,
    pub(crate) size: u32,
    pub(crate) mtime: FileTime,
}

/// A checkpoint's skip record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SkipRecord {
    /// The files not versioned, sorted by path bytes.
    pub(crate) skipped: Vec<SkipEntry>,
    /// The versioned files that are cloud-only now and were not read, sorted by path bytes; each
    /// reason is `CloudOnly`.
    pub(crate) stale: Vec<SkipEntry>,
    /// The folders that did not list, relative to the matter folder, sorted.
    pub(crate) unread: Vec<String>,
}

/// The reason a skipped file's size says when the walk did not: over `cap`, else cloud-only.
pub(crate) fn reason_by_size(size: u32, cap: u64) -> SkippedFileReason {
    if u64::from(size) > cap {
        SkippedFileReason::OverSizeCap
    } else {
        SkippedFileReason::CloudOnly
    }
}

impl SkipRecord {
    /// The record of a walk: its skipped `entries` (sorted by path bytes), each with the reason
    /// `reason_of` gives, and its `unread` folders.
    pub(crate) fn of(
        entries: &[CacheEntry],
        unread: &[String],
        reason_of: &dyn Fn(&CacheEntry) -> SkippedFileReason,
    ) -> Self {
        let skip = |entry: &CacheEntry, reason| SkipEntry {
            path: entry.path.clone(),
            reason,
            size: entry.stat.size,
            mtime: entry.stat.mtime,
        };
        let mut record = Self::default();
        for entry in entries {
            match entry.state {
                EntryState::Skipped => record.skipped.push(skip(entry, reason_of(entry))),
                EntryState::Unread(_) => {
                    record.stale.push(skip(entry, SkippedFileReason::CloudOnly));
                }
                EntryState::Read(_) => {}
            }
        }
        record.unread = unread.to_vec();
        record.unread.sort_unstable();
        record.unread.dedup();
        record
    }

    /// The record of a walk's skipped `entries` (sorted by path bytes) and `unread` folders, the
    /// reason of each from `reasons`, or for one kept from the cache, from its size and `cap`.
    pub(crate) fn of_walk(
        entries: &[CacheEntry],
        unread: &[String],
        reasons: &BTreeMap<String, SkippedFileReason>,
        cap: u64,
    ) -> Self {
        Self::of(entries, unread, &|entry| {
            reasons
                .get(entry.path.as_str())
                .copied()
                .unwrap_or_else(|| reason_by_size(entry.stat.size, cap))
        })
    }

    /// The record from the stat cache alone, for a tip written before records were.
    pub(crate) fn of_cache(entries: &[CacheEntry], cap: u64) -> Self {
        Self::of(entries, &[], &|entry| reason_by_size(entry.stat.size, cap))
    }

    /// Whether it records nothing: the commit says `none`.
    pub(crate) fn is_empty(&self) -> bool {
        self.skipped.is_empty() && self.stale.is_empty() && self.unread.is_empty()
    }

    /// The skipped file at `path`, if any.
    pub(crate) fn skipped_at(&self, path: &str) -> Option<&SkipEntry> {
        find_by_path(&self.skipped, path, |entry| entry.path.as_str())
    }

    /// The versioned file at `path` that was cloud-only and not read, if any.
    pub(crate) fn stale_at(&self, path: &str) -> Option<&SkipEntry> {
        find_by_path(&self.stale, path, |entry| entry.path.as_str())
    }

    /// Whether `path` lies under a folder that did not list.
    pub(crate) fn under_unread(&self, path: &str) -> bool {
        self.unread.iter().any(|folder| {
            path.strip_prefix(folder.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        })
    }

    /// The blob's text.
    pub(crate) fn encode(&self) -> String {
        let mut lines: Vec<(&str, String)> = Vec::new();
        for entry in &self.skipped {
            let line = format!(
                "skipped {} {} {}.{:09} {}",
                reason_name(entry.reason),
                entry.size,
                entry.mtime.secs,
                entry.mtime.nanos,
                entry.path
            );
            lines.push((entry.path.as_str(), line));
        }
        for entry in &self.stale {
            let line = format!(
                "stale {} {}.{:09} {}",
                entry.size, entry.mtime.secs, entry.mtime.nanos, entry.path
            );
            lines.push((entry.path.as_str(), line));
        }
        for folder in &self.unread {
            lines.push((folder.as_str(), format!("unread {folder}")));
        }
        lines.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let mut text = String::from(HEADER);
        text.push('\n');
        for (_, line) in lines {
            text.push_str(&line);
            text.push('\n');
        }
        text
    }

    /// The record a blob's text holds; `None` when its header is not one this version reads
    /// (then nothing about the turn's start is known). A line that does not read is left out.
    pub(crate) fn decode(text: &str) -> Option<Self> {
        let mut lines = text.split('\n');
        if lines.next() != Some(HEADER) {
            return None;
        }
        let mut record = Self::default();
        for line in lines {
            if let Some(rest) = line.strip_prefix("skipped ") {
                if let Some(entry) = skipped_line(rest) {
                    record.skipped.push(entry);
                }
            } else if let Some(rest) = line.strip_prefix("stale ") {
                if let Some(entry) = skipped_line(&format!("cloud-only {rest}")) {
                    record.stale.push(entry);
                }
            } else if let Some(folder) = line.strip_prefix("unread ")
                && MatterPath::new(folder).is_ok()
            {
                record.unread.push(folder.to_owned());
            }
        }
        record
            .skipped
            .sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
        record
            .stale
            .sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
        record.unread.sort_unstable();
        Some(record)
    }
}

/// The record `skips` names, read from `history` and decoded; `None` when the commit has none
/// or it does not read or decode (warned): nothing is known then.
pub(crate) fn read(history: &impl HistoryStore, skips: SkipsRef) -> Option<SkipRecord> {
    let blob = match skips {
        SkipsRef::Unrecorded => return None,
        SkipsRef::Empty => return Some(SkipRecord::default()),
        SkipsRef::Blob(blob) => blob,
    };
    let mut bytes = Vec::new();
    if let Err(err) = history.read_blob(blob, "skip record", &mut bytes) {
        tracing::warn!(code = %err.code(), "a skip record did not read");
        return None;
    }
    let record = String::from_utf8(bytes)
        .ok()
        .and_then(|text| SkipRecord::decode(&text));
    if record.is_none() {
        tracing::warn!("a skip record did not decode");
    }
    record
}

/// `<reason> <size> <secs>.<nanos> <path>`.
fn skipped_line(rest: &str) -> Option<SkipEntry> {
    let mut parts = rest.splitn(4, ' ');
    let reason = reason_from(parts.next()?);
    let size = parts.next()?.parse().ok()?;
    let (secs, nanos) = parts.next()?.split_once('.')?;
    let mtime = FileTime {
        secs: secs.parse().ok()?,
        nanos: nanos.parse().ok()?,
    };
    let path = MatterPath::new(parts.next()?).ok()?;
    Some(SkipEntry {
        path,
        reason,
        size,
        mtime,
    })
}

/// The spelling of each reason, in the record and in the first-read trailers.
const REASONS: [(SkippedFileReason, &str); 3] = [
    (SkippedFileReason::CloudOnly, "cloud-only"),
    (SkippedFileReason::OverSizeCap, "over-size-cap"),
    (SkippedFileReason::UnreadFolder, "unread-folder"),
];

/// A reason's spelling; `unknown` for one with none.
pub(crate) fn reason_name(reason: SkippedFileReason) -> &'static str {
    REASONS
        .iter()
        .find(|(known, _)| *known == reason)
        .map_or("unknown", |(_, name)| *name)
}

/// The reason a spelling names; `Unknown` for one this version does not know.
pub(crate) fn reason_from(name: &str) -> SkippedFileReason {
    REASONS
        .iter()
        .find(|(_, known)| *known == name)
        .map_or(SkippedFileReason::Unknown, |(reason, _)| *reason)
}

/// Whether `stat` has the size and modification time of `entry`.
pub(crate) fn same_stat(entry: &SkipEntry, stat: &Stat) -> bool {
    entry.size == stat.size && entry.mtime == stat.mtime
}

#[cfg(test)]
mod tests;

//! A matter's papers: the files of its folder as they are now, by the checkpoint walk's rules,
//! for the matter's file list. Only stats: no file is opened, read or written.

use std::path::Path;

use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason, Timestamp};

use crate::Error;
use crate::no_download::NoDownload;
use crate::stat::FileTime;
use crate::types::Settings;
use crate::walk::{self, Found, FoundKind};

/// What [`crate::Matter::papers`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperList {
    /// The files Clauseo versions, sorted by path bytes.
    pub papers: Vec<Paper>,
    /// The files it does not version (over the size cap, cloud-only), sorted by path bytes.
    pub skipped: Vec<SkippedFile>,
    /// The subfolders that could not be read (permission denied, or not downloaded), sorted by
    /// path bytes: what they hold is not listed, though a checkpoint keeps it as it was.
    pub unread: Vec<MatterPath>,
    /// Whether the folder holds more than the limit: `papers`, `skipped` and `unread`
    /// together are then the first `limit` entries by path, a subfolder counted where
    /// `<folder>/` sorts.
    pub truncated: bool,
}

/// One file Clauseo versions, as the folder holds it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paper {
    /// The file, relative to the matter folder.
    pub path: MatterPath,
    /// Its size in bytes.
    pub size: u64,
    /// Its modification time, to the millisecond; 0 when the file system has none or it is
    /// before 1970.
    pub modified: Timestamp,
}

/// See [`crate::Matter::papers`].
pub(crate) fn papers(folder: &Path, settings: &Settings, limit: usize) -> Result<PaperList, Error> {
    let guard = NoDownload::begin(settings.probe.as_ref());
    let cap = settings.limits.effective_cap();
    let listing = walk::walk_first(folder, &guard, cap, limit)?;
    let mut unread: Vec<MatterPath> = listing
        .unread
        .iter()
        .filter_map(|dir| MatterPath::new(dir).ok())
        .collect();
    let truncated = listing.files.len() + unread.len() > limit;
    let mut files = listing.files;
    if truncated {
        drop_last(&mut files, &mut unread);
    }
    unread.sort_unstable_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    let mut papers = Vec::with_capacity(files.len());
    let mut skipped = Vec::new();
    for file in files {
        let reason = match file.kind {
            FoundKind::File => {
                papers.push(Paper {
                    path: file.path,
                    size: file.len,
                    modified: millis(file.stat.mtime),
                });
                continue;
            }
            FoundKind::CloudOnly => SkippedFileReason::CloudOnly,
            FoundKind::OverCap => SkippedFileReason::OverSizeCap,
        };
        skipped.push(SkippedFile {
            path: file.path,
            reason,
        });
    }
    Ok(PaperList {
        papers,
        skipped,
        unread,
        truncated,
    })
}

/// Drops the entry that sorts last of `files` (sorted) and `unread` (folders, each sorting as
/// `<folder>/`): the one the walk found past the limit.
fn drop_last(files: &mut Vec<Found>, unread: &mut Vec<MatterPath>) {
    let folder_key = |dir: &MatterPath| dir.as_str().bytes().chain(*b"/").collect::<Vec<u8>>();
    let last_folder = unread
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| folder_key(a).cmp(&folder_key(b)))
        .map(|(at, dir)| (at, folder_key(dir)));
    match (files.last(), last_folder) {
        (Some(file), Some((at, key))) => {
            if key.as_slice() > file.path.as_str().as_bytes() {
                unread.remove(at);
            } else {
                files.pop();
            }
        }
        (Some(_), None) => {
            files.pop();
        }
        (None, Some((at, _))) => {
            unread.remove(at);
        }
        (None, None) => {}
    }
}

/// `time` in milliseconds since the Unix epoch. Its seconds are 32 bits, so it always fits.
fn millis(time: FileTime) -> Timestamp {
    let millis = i64::from(time.secs) * 1000 + i64::from(time.nanos / 1_000_000);
    Timestamp::new(millis).unwrap_or(Timestamp::ZERO)
}

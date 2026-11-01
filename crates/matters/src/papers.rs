//! A matter's papers: the files of its folder as they are now, by the checkpoint walk's rules,
//! for the matter's file list. Only stats: no file is opened, read or written.

use std::path::Path;

use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason, Timestamp};

use crate::Error;
use crate::no_download::NoDownload;
use crate::stat::FileTime;
use crate::types::Settings;
use crate::walk::{self, FoundKind};

/// What [`crate::Matter::papers`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperList {
    /// The files Clauseo versions, sorted by path bytes.
    pub papers: Vec<Paper>,
    /// The files it does not version (over the size cap, cloud-only), sorted by path bytes.
    pub skipped: Vec<SkippedFile>,
    /// Whether the folder holds more files than the limit: `papers` and `skipped` together
    /// are then the first `limit` files by path.
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
    let truncated = listing.files.len() > limit;
    let mut papers = Vec::with_capacity(listing.files.len().min(limit));
    let mut skipped = Vec::new();
    for file in listing.files.into_iter().take(limit) {
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
        truncated,
    })
}

/// `time` in milliseconds since the Unix epoch. Its seconds are 32 bits, so it always fits.
fn millis(time: FileTime) -> Timestamp {
    let millis = i64::from(time.secs) * 1000 + i64::from(time.nanos / 1_000_000);
    Timestamp::new(millis).unwrap_or(Timestamp::ZERO)
}

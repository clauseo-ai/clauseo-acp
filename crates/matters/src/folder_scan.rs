//! The folder scan before a matter: what an existing folder holds, counted as the walk goes, so
//! the window can show it before the lawyer makes the folder a matter. Nothing is written.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use clauseo_protocol::MatterPath;

use crate::no_download::NoDownload;
use crate::walk::{self, FoundKind};
use crate::{Error, Settings, refuse};

/// How far a [`scan_folder`] has got, after each file the walk reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanProgress<'a> {
    /// Files to version found so far: cloud-only and over-cap files are not counted.
    pub files: u64,
    /// Their total size in bytes.
    pub bytes: u64,
    /// Cloud-only files found so far, counted from their metadata and never read.
    pub cloud_only: u64,
    /// The file the walk just reached, relative to the folder. The walk takes its stat and
    /// never opens it.
    pub now_reading: &'a MatterPath,
}

/// What a finished [`scan_folder`] found: `files` and `bytes` are what the root checkpoint of
/// an import of the same folder versions (`ImportSummary::files` and `bytes`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderTotals {
    /// Files to version.
    pub files: u64,
    /// Their total size in bytes.
    pub bytes: u64,
    /// Cloud-only files, never read.
    pub cloud_only: u64,
}

/// A [`scan_folder`]'s result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderScan {
    /// The walk finished.
    Done(FolderTotals),
    /// The cancel flag was set.
    Cancelled,
}

/// Scans `folder` before it becomes a matter: refuses it as
/// [`Matter::import`](crate::Matter::import) would (a drive root, an entry of `refused` or an
/// ancestor of one, the matters root or a refused tree), then walks it by the import's rules
/// (the exclusion list, the settings' probe and size cap) under a [`NoDownload`] guard.
/// `progress` gets the running counts and the file reached after each file. Returns
/// [`FolderScan::Cancelled`] once `cancel` is set (the walk checks it before each folder, entry
/// and stat). Opens no file and writes nothing; a cloud-only file is counted from its metadata.
pub fn scan_folder(
    folder: &Path,
    refused: &[PathBuf],
    settings: &Settings,
    progress: &mut dyn FnMut(ScanProgress<'_>),
    cancel: &AtomicBool,
) -> Result<FolderScan, Error> {
    refuse::check_folder(folder, refused)?;
    refuse::check_new_matter(folder, settings)?;
    let mut totals = FolderTotals::default();
    let guard = NoDownload::begin(settings.probe.as_ref());
    let walked = walk::walk(
        folder,
        &guard,
        settings.limits.effective_cap(),
        Some(cancel),
        &mut |found| {
            match found.kind {
                FoundKind::File => {
                    totals.files += 1;
                    totals.bytes = totals.bytes.saturating_add(found.len);
                }
                FoundKind::CloudOnly => totals.cloud_only += 1,
                FoundKind::OverCap => {}
            }
            progress(ScanProgress {
                files: totals.files,
                bytes: totals.bytes,
                cloud_only: totals.cloud_only,
                now_reading: &found.path,
            });
        },
    )?;
    Ok(match walked {
        Some(_) => FolderScan::Done(totals),
        None => FolderScan::Cancelled,
    })
}

#[cfg(test)]
mod tests;

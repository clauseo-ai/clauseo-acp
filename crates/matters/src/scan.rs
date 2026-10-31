//! The import scan: what importing a folder would take, before anything is written.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::no_download::NoDownload;
use crate::walk::{self, FoundKind};
use crate::{
    Error, ImportWarning, LARGEST_FILES, LargeFile, Limits, Progress, ScanReport, Scanned,
    Settings, refuse,
};

/// Scans `folder` for an import: refuses it as [`Matter::import`](crate::Matter::import) would
/// (a drive root, or an entry of `refused` or an ancestor of one), then walks it with the
/// settings' probe and size cap. `progress` gets the running count of files seen and their bytes
/// after each file. Returns [`Scanned::Cancelled`] once `cancel` is set. Reads no file's content
/// and writes nothing; lists folders under a [`NoDownload`] guard, so a folder that is not
/// downloaded stays so.
pub fn scan(
    folder: &Path,
    refused: &[PathBuf],
    settings: &Settings,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Scanned, Error> {
    refuse::check_folder(folder, refused)?;
    refuse::check_new_matter(folder, &settings.matters_root)?;
    let mut seen = Progress { files: 0, bytes: 0 };
    let guard = NoDownload::begin(settings.probe.as_ref());
    let walked = walk::walk(
        folder,
        &guard,
        settings.limits.effective_cap(),
        Some(cancel),
        &mut |found| {
            seen.files += 1;
            seen.bytes = seen.bytes.saturating_add(found.len);
            progress(seen);
        },
    )?;
    let Some(listing) = walked else {
        return Ok(Scanned::Cancelled);
    };
    let mut report = ScanReport {
        files: 0,
        bytes: 0,
        cloud_only: 0,
        over_cap: 0,
        largest: Vec::new(),
        warnings: Vec::new(),
    };
    let mut largest = Vec::new();
    for file in listing.files {
        match file.kind {
            FoundKind::File => {
                report.files += 1;
                report.bytes = report.bytes.saturating_add(file.len);
            }
            FoundKind::CloudOnly => {
                report.cloud_only += 1;
                continue;
            }
            FoundKind::OverCap => report.over_cap += 1,
        }
        largest.push(LargeFile {
            path: file.path,
            bytes: file.len,
        });
    }
    // Largest first; the walk's order (path bytes) breaks ties, and the sort is stable.
    largest.sort_by_key(|file| std::cmp::Reverse(file.bytes));
    largest.truncate(LARGEST_FILES);
    report.largest = largest;
    report.warnings = warnings(folder, report.bytes, &settings.limits);
    Ok(Scanned::Done(report))
}

/// The import warnings for `folder` holding `bytes` to version: [`ImportWarning::UncShare`] for
/// a UNC path, then [`ImportWarning::OverThreshold`] above `limits.import_warn_bytes`.
pub(crate) fn warnings(folder: &Path, bytes: u64, limits: &Limits) -> Vec<ImportWarning> {
    let mut warnings = Vec::new();
    if refuse::is_unc(folder) {
        warnings.push(ImportWarning::UncShare);
    }
    if bytes > limits.import_warn_bytes {
        warnings.push(ImportWarning::OverThreshold { bytes });
    }
    warnings
}

#[cfg(test)]
mod tests;

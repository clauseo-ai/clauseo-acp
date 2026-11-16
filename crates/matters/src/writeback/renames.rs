//! Step 4 of [`super::apply`]: every staged file renamed over its target, or written in place
//! where the rename is refused (`write::Staged::rename_or_in_place`, macOS), and a file refused
//! either way passed over, so the files after it are still written; then each folder that got
//! one is synced once.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterPath;

use crate::checkpoint::Core;
use crate::{Error, RefusedBecause, write};

/// What the renames did besides their folders: the caller's tallies.
pub(super) struct Tally<'a> {
    /// A delete or a rename made: a failure from here on leaves the folder changed.
    pub(super) any_change: &'a mut bool,
    /// What of the old files' metadata the new ones did not keep.
    pub(super) lost: &'a mut write::Lost,
    /// Files renamed into place whose step after the rename failed.
    pub(super) owner_only: &'a mut Vec<MatterPath>,
    /// Files neither renamed over nor written in place, and why: each as it was.
    pub(super) refused: &'a mut Vec<(MatterPath, RefusedBecause)>,
    /// How many files the write-back writes or deletes: [`Error::PartlyWritten`]'s `of`.
    pub(super) of: usize,
}

/// Renames (or writes in place) every file of `staged`, in order, and syncs each folder that got
/// one. A refused file is counted in `tally.refused` and the next one goes on; any other failure
/// stops there: the files before it stay, their folders synced as far as the OS lets, and the
/// temp files after it, and its own, are removed as each `Staged` drops. The error then comes
/// with whether the folder changed; a write in place that failed part way is
/// [`Error::PartlyWritten`], saying whether the file was put back.
pub(super) fn rename_all(
    core: &Core,
    folder: &Path,
    staged: Vec<(&MatterPath, write::Staged)>,
    tally: &mut Tally<'_>,
) -> Result<(), (Error, bool)> {
    let mut synced = BTreeSet::new();
    for (path, file) in staged {
        #[cfg(feature = "test-support")]
        if let Some(because) = crate::test_support::refused_at_write(&folder.join(path.as_str())) {
            tally.refused.push((path.clone(), because));
            drop(file);
            continue;
        }
        #[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
        let renamed = file.rename_or_in_place(&mut super::saved::Saved {
            core,
            path,
            blob: None,
        });
        #[cfg(not(any(target_os = "macos", all(test, target_os = "linux"))))]
        let renamed = {
            let _ = core;
            file.rename()
        };
        let failed = match renamed {
            Ok(renamed) => {
                *tally.any_change = true;
                tally.lost.add(renamed.lost);
                if renamed.owner_only {
                    tally.owner_only.push(path.clone());
                }
                synced.insert(renamed.dir);
                continue;
            }
            Err(failed) => failed,
        };
        if let Some(put_back) = failed.torn {
            sync_all(&synced);
            let error = partly_written(failed.error, path, put_back, tally.of);
            return Err((error, *tally.any_change || failed.changed));
        }
        // The old file gone: not a file held open, and changed.
        let error = if failed.changed {
            failed.error
        } else {
            super::held_or(folder, path, failed.error)
        };
        if let Some(because) = refused(&error, failed.refused, failed.changed) {
            tally.refused.push((path.clone(), because));
            continue;
        }
        sync_all(&synced);
        return Err((error, *tally.any_change || failed.changed));
    }
    for dir in &synced {
        crate::fsync::sync_dir(dir).map_err(|err| (Error::io("sync", dir, err), true))?;
    }
    Ok(())
}

/// [`Error::PartlyWritten`] for the matter's `path`, whose write in place failed with `error`
/// (an [`Error::Io`], whose OS error it keeps) after it began.
fn partly_written(error: Error, path: &MatterPath, put_back: bool, of: usize) -> Error {
    let source = if let Error::Io { source, .. } = error {
        source
    } else {
        io::Error::other("write in place")
    };
    Error::PartlyWritten {
        path: path.clone(),
        put_back,
        of,
        source,
    }
}

/// Why the file whose rename failed with `error` is passed over: the write in place `refused`
/// it, or (with no write in place tried) the rename was refused for its permissions. `None` for
/// a failure that stops the renames: a file held open, a folder changed, any other error.
pub(super) fn refused(
    error: &Error,
    refused: Option<RefusedBecause>,
    changed: bool,
) -> Option<RefusedBecause> {
    if changed {
        return None;
    }
    refused.or_else(|| match error {
        Error::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied => {
            Some(RefusedBecause::Permissions)
        }
        Error::Io { .. }
        | Error::FolderMissing { .. }
        | Error::FolderRefused { .. }
        | Error::FileLocked { .. }
        | Error::FilesRefused { .. }
        | Error::PartlyWritten { .. }
        | Error::ChangedSince { .. }
        | Error::NotVersioned { .. }
        | Error::UnknownCheckpoint { .. }
        | Error::DuplicateCheckpoint { .. }
        | Error::MissingDecision { .. }
        | Error::UnexpectedDecision { .. }
        | Error::UnknownFile { .. }
        | Error::DuplicateFile { .. }
        | Error::NoFilesChosen
        | Error::FileFolderClash { .. }
        | Error::FolderExists { .. }
        | Error::HistoryExists { .. }
        | Error::MarkerNotBuilt
        | Error::History { .. } => None,
    })
}

/// Syncs each folder of `synced` that got a rename, before an error returns; a failure is
/// logged, as the error that stopped the renames is the one returned.
fn sync_all(synced: &BTreeSet<PathBuf>) {
    for dir in synced {
        if let Err(err) = crate::fsync::sync_dir(dir) {
            tracing::warn!(error = %err, "a renamed file's folder not synced");
        }
    }
}

//! Windows: a lawyer's file replaced by `ReplaceFileW` ([`super::windows::replace_file`]), which
//! keeps the old file's attributes, ACL, alternate data streams and creation time, and what to do
//! when it fails. The call is passed in, so the steps after each failure are tested on every OS
//! with the failure simulated.
//!
//! The call is given a backup name in the `.clauseo-` temp namespace (excluded from every
//! checkpoint). By its documentation, after a failure:
//! - 1175 `ERROR_UNABLE_TO_REMOVE_REPLACED`: nothing moved; the target is as it was.
//! - 1176 `ERROR_UNABLE_TO_MOVE_REPLACEMENT`: with a backup name, both files keep their names.
//! - 1177 `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2`: the old file is under the backup name and the
//!   target's name is free; it is renamed back.
//!
//! The steps go by what is on disk after the call, not by the code alone: a target still there
//! gets the plain rename (whose error, such as a sharing violation, is the one returned); a target
//! under the backup name is put back; a target gone with no backup gets the plain rename. When the
//! target is not there at the end, the temp file (the new content) is never removed: the caller
//! keeps it and reports that the folder changed.
//!
//! A symlink or junction never reaches this: `writeback::writable` writes only a regular file or
//! a missing one, and the stat check before the renames fails a target replaced by one since.

use std::io;
use std::path::{Path, PathBuf};

use super::{Lost, TEMP_TRIES, remove_file, rename};

/// What [`next_step`] chose after the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// Replaced: the backup (the old file) is removed.
    Done,
    /// The plain rename of the temp file over the target, or to its free name.
    Rename,
    /// The old file is under the backup name: renamed back to the target.
    PutBack,
}

/// The step after a `ReplaceFileW` that `failed` or not, with the target and the backup there
/// or not.
pub(super) const fn next_step(failed: bool, target_there: bool, backup_there: bool) -> Step {
    if !failed {
        Step::Done
    } else if !target_there && backup_there {
        Step::PutBack
    } else {
        Step::Rename
    }
}

/// Why [`replace_keeping`] failed.
#[derive(Debug)]
pub(super) enum Failed {
    /// The target is in place (as it was, or put back): the temp file may go.
    Unchanged(io::Error),
    /// The target is not there: the temp file holds the new content and must be kept.
    TargetGone(io::Error),
}

impl Failed {
    /// The error, whichever way it failed.
    pub(super) fn into_error(self) -> io::Error {
        match self {
            Self::Unchanged(err) | Self::TargetGone(err) => err,
        }
    }
}

/// Puts the temp file `temp` in the place of `target` with `replace(target, temp, backup)`, then
/// the step [`next_step`] chooses; what was lost of the old file's metadata.
pub(super) fn replace_keeping(
    temp: &Path,
    target: &Path,
    replace: impl FnOnce(&Path, &Path, &Path) -> io::Result<()>,
) -> Result<Lost, Failed> {
    let dir = target.parent().unwrap_or(target);
    let backup = free_name(dir).map_err(Failed::Unchanged)?;
    let result = replace(target, temp, &backup);
    let code = result.as_ref().err().and_then(io::Error::raw_os_error);
    let target_there = there(target);
    let backup_there = there(&backup);
    match next_step(result.is_err(), target_there, backup_there) {
        Step::Done => {
            if backup_there && let Err(err) = remove_file(&backup) {
                tracing::warn!(error = %err, "backup of a replaced file left behind");
            }
            Ok(Lost::default())
        }
        Step::Rename => match rename(temp, target) {
            // Over the old file: its metadata is lost. To a free name: nothing to keep.
            Ok(()) if target_there => Ok(Lost::one(code)),
            Ok(()) => Ok(Lost::default()),
            Err(err) if there(target) => Err(Failed::Unchanged(err)),
            Err(err) => Err(Failed::TargetGone(err)),
        },
        Step::PutBack => match rename(&backup, target) {
            Ok(()) => Err(Failed::Unchanged(
                result.err().unwrap_or_else(|| io::ErrorKind::Other.into()),
            )),
            Err(err) => Err(Failed::TargetGone(err)),
        },
    }
}

/// Whether something is at `path`; an error other than "not found" counts as there.
fn there(path: &Path) -> bool {
    !matches!(std::fs::symlink_metadata(path), Err(err) if err.kind() == io::ErrorKind::NotFound)
}

/// The first `.clauseo-<n>.tmp` name in `dir` with nothing at it, for the backup.
fn free_name(dir: &Path) -> io::Result<PathBuf> {
    for n in 0..TEMP_TRIES {
        let path = dir.join(format!(".clauseo-{n}.tmp"));
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(path),
            Err(err) => return Err(err),
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

#[cfg(test)]
mod tests;

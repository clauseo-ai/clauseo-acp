//! The std calls that change files: the one place in `profile` that may call them. The
//! workspace `clippy.toml` bans each of them everywhere else, and the expect below covers only
//! this file.
//!
//! What changes: the skills folders the profile owns (Claude's plugin under
//! `<data>/profile/claude-plugin`, Codex's `<agent home>/skills`), and in them only the folders
//! of the skills a render is given, their manifest and the plugin's `plugin.json`.

#![expect(
    clippy::disallowed_methods,
    reason = "profile writes files here alone; clippy.toml bans std's writes elsewhere"
)]

use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::ProfileError;

/// Numbers the temp files of one process, so two calls never pick the same name.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// The suffix of every temp file [`replace_if_differs`] makes: `.<name>.<pid>-<n>.tmp`.
const TEMP_SUFFIX: &str = ".tmp";

/// Whether `name` is one of [`replace_if_differs`]'s temp files: `.`, a name, `.`, digits,
/// `-`, digits, `.tmp`.
pub(crate) fn is_temp_name(name: &str) -> bool {
    let Some(inner) = name
        .strip_prefix('.')
        .and_then(|rest| rest.strip_suffix(TEMP_SUFFIX))
    else {
        return false;
    };
    let Some((stem, number)) = inner.rsplit_once('.') else {
        return false;
    };
    let Some((pid, n)) = number.split_once('-') else {
        return false;
    };
    !stem.is_empty()
        && [pid, n]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Removes the temp files a crashed write left in `dir` and, when `deep`, in its subfolders,
/// never following a symlink. Only names [`is_temp_name`] matches go. A missing `dir` is no
/// error; a file that will not go is logged and left.
pub(crate) fn sweep_temps(dir: &Path, deep: bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() && deep {
            sweep_temps(&path, true);
        } else if kind.is_file()
            && entry.file_name().to_str().is_some_and(is_temp_name)
            && let Err(err) = std::fs::remove_file(&path)
        {
            tracing::warn!(path = %path.display(), %err, "a leftover temp file stays");
        }
    }
}

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) -> Result<(), ProfileError> {
    std::fs::create_dir_all(path).map_err(|err| ProfileError::io("create", path, err))
}

/// `fs::remove_file`; a missing file is no error.
pub(crate) fn remove_file(path: &Path) -> Result<(), ProfileError> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => {
            Err(ProfileError::io("remove", path, err))
        }
        _ => Ok(()),
    }
}

/// `fs::remove_dir` of a folder only when it is empty; a missing or non-empty folder is left
/// as it is, with no error.
pub(crate) fn remove_empty_dir(path: &Path) {
    let empty = std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none());
    if empty && let Err(err) = std::fs::remove_dir(path) {
        tracing::warn!(path = %path.display(), %err, "an emptied skill folder stays");
    }
}

/// Makes `target` hold exactly `bytes`, making its folder if missing: a new temp file beside it,
/// written, synced, then renamed over it, so a reader never sees half a file. Writes only when
/// the file differs (a missing file differs). Returns whether it wrote.
pub(crate) fn replace_if_differs(target: &Path, bytes: &[u8]) -> Result<bool, ProfileError> {
    if std::fs::read(target).is_ok_and(|held| held == bytes) {
        return Ok(false);
    }
    let dir = target
        .parent()
        .ok_or_else(|| ProfileError::io("replace", target, io::ErrorKind::InvalidInput.into()))?;
    create_dir_all(dir)?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(".{name}.{}-{n}{TEMP_SUFFIX}", std::process::id()));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| std::fs::rename(&temp, target));
    if let Err(err) = written {
        let _gone = std::fs::remove_file(&temp);
        return Err(ProfileError::io("write", target, err));
    }
    sync_dir(dir);
    Ok(true)
}

/// Syncs the folder `dir`, so a rename in it survives a power cut. A failure is no error.
#[cfg(unix)]
fn sync_dir(dir: &Path) {
    let _synced = std::fs::File::open(dir).and_then(|folder| folder.sync_all());
}

/// Nothing on Windows: a folder cannot be opened as a file there, and NTFS journals the rename.
#[cfg(not(unix))]
fn sync_dir(_dir: &Path) {}

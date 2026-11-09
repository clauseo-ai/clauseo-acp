//! The std calls that change files: the one place in `profile` that may call them. The
//! workspace `clippy.toml` bans each of them everywhere else, and the expect below covers only
//! this file.
//!
//! What changes: the skills folders the profile owns (Claude's plugin under
//! `<data>/profile/claude-plugin`, Codex's `<agent home>/skills`), and in them only the folders
//! of the skills a render is given, their manifest and the plugin's `plugin.json`; and for the
//! skill packs (decision record 0040), a launch's own pack plugin, `run/<session id>/skill-packs`;
//! and the agents' shell commands in `<data>/tools` ([`crate::tools`]).

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
            tracing::warn!(%err, "a leftover temp file stays");
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
        tracing::warn!(%err, "an emptied skill folder stays");
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

/// Makes `target` a program holding exactly `bytes` (mode 0755 on Unix), as
/// [`replace_if_differs`] does: a temp file beside it, written, synced, then renamed over it.
/// The caller has decided it differs. On Windows a program that is running cannot be renamed
/// over, but it can be renamed: the old one goes aside as `.<name>.<pid>-<n>.old` first, which a
/// later call's [`sweep_old`] removes once nothing runs it.
pub(crate) fn replace_program(target: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
    let dir = target
        .parent()
        .ok_or_else(|| ProfileError::io("replace", target, io::ErrorKind::InvalidInput.into()))?;
    create_dir_all(dir)?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(".{name}.{}-{n}{TEMP_SUFFIX}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o755);
    let written = options
        .open(&temp)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| match std::fs::rename(&temp, target) {
            Err(err) if cfg!(windows) && target.exists() => {
                let old = dir.join(format!(".{name}.{}-{n}{OLD_SUFFIX}", std::process::id()));
                std::fs::rename(target, &old)
                    .and_then(|()| std::fs::rename(&temp, target))
                    .map_err(|_| err)
            }
            other => other,
        });
    if let Err(err) = written {
        let _gone = std::fs::remove_file(&temp);
        return Err(ProfileError::io("write", target, err));
    }
    sync_dir(dir);
    Ok(())
}

/// The suffix of a program [`replace_program`] set aside on Windows.
const OLD_SUFFIX: &str = ".old";

/// Removes the programs [`replace_program`] set aside in `dir`, and the temp files a crashed
/// write left there; one still running stays, quietly, for the next call.
pub(crate) fn sweep_old(dir: &Path) {
    sweep_temps(dir, false);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry.file_name().to_str().is_some_and(|name| {
            name.strip_suffix(OLD_SUFFIX)
                .is_some_and(|stem| is_temp_name(&format!("{stem}{TEMP_SUFFIX}")))
        });
        if old && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _still_running = std::fs::remove_file(entry.path());
        }
    }
}

/// `fs::create_dir`: a new folder, an error when `path` already exists.
pub(crate) fn create_dir(path: &Path) -> Result<(), ProfileError> {
    std::fs::create_dir(path).map_err(|err| ProfileError::io("create", path, err))
}

/// A new file at `path` holding `bytes`, its folder made if missing, an error when the file
/// already exists. Opened with the default mode, so no exec bit. Not synced: it is for a
/// launch's disposable pack plugin, which a crash leaves for the run folder's cleanup.
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
    if let Some(dir) = path.parent() {
        create_dir_all(dir)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|err| ProfileError::io("write", path, err))
}

/// `fs::remove_dir_all`, which never follows a symlink inside; a missing folder is no error,
/// and any other is logged and left.
pub(crate) fn remove_dir_all(path: &Path) {
    match std::fs::remove_dir_all(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => {
            tracing::warn!(%err, "a leftover folder stays");
        }
        _ => {}
    }
}

/// Syncs the folder `dir`, so a rename in it survives a power cut. A failure is no error.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) {
    let _synced = std::fs::File::open(dir).and_then(|folder| folder.sync_all());
}

/// Nothing on Windows: a folder cannot be opened as a file there, and NTFS journals the rename.
#[cfg(not(unix))]
pub(crate) fn sync_dir(_dir: &Path) {}

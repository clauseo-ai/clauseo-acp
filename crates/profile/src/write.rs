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
    let temp = dir.join(format!(".{name}.{}-{n}.tmp", std::process::id()));
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
    Ok(true)
}

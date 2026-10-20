//! Replacing a file's bytes through a temporary file in the same folder, renamed over it, so a
//! kill never leaves half a file. The temporary file is named as `ui/app/scripts/guard.mjs` names
//! its own: `.<name>.<tool>.tmp`.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The temporary file a write by `tool` to `path` goes through: `.<name>.<tool>.tmp`, in the
/// same folder.
pub(super) fn tmp_of(path: &Path, tool: &str) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.{tool}.tmp"))
}

/// Why a replace failed: the OS error, and the temporary file when it could not be deleted.
#[derive(Debug)]
pub(super) struct ReplaceFailure {
    pub error: io::Error,
    pub tmp_left: Option<PathBuf>,
}

/// Writes `bytes` to `tmp_of(path, tool)`, syncs it, gives it `path`'s permissions when `path`
/// exists, and renames it over `path`. When this fails, `path` is as it was and the temporary file
/// is deleted (or named in the failure when that fails too).
pub(super) fn replace(path: &Path, tool: &str, bytes: &[u8]) -> Result<(), ReplaceFailure> {
    let tmp = tmp_of(path, tool);
    let written = write_synced(&tmp, bytes)
        .and_then(|()| match fs::metadata(path) {
            Ok(meta) => fs::set_permissions(&tmp, meta.permissions()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        })
        .and_then(|()| fs::rename(&tmp, path));
    written.map_err(|error| {
        let gone = match fs::remove_file(&tmp) {
            Ok(()) => true,
            Err(missing) => missing.kind() == io::ErrorKind::NotFound,
        };
        ReplaceFailure {
            error,
            tmp_left: (!gone).then_some(tmp),
        }
    })
}

/// Writes `bytes` to `tmp`, replacing what is there, and syncs it.
fn write_synced(tmp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(tmp)?;
    file.write_all(bytes).and_then(|()| file.sync_all())
}

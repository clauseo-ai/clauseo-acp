//! The std calls that change files: the one place in `counts` that may call them. The workspace
//! `clippy.toml` bans each of them everywhere else, and the expect below covers only this file.
//!
//! What changes: `<data_dir>/first_act_done` (written once, never deleted) and everything under
//! `<data_dir>/counts/` (written whole through a temp file and a rename, and deleted whole when
//! counts turn off).

#![expect(
    clippy::disallowed_methods,
    reason = "counts writes files here alone; clippy.toml bans std's writes elsewhere"
)]

use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Numbers the temp files of one process, so two calls never pick the same name.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Makes `target` hold exactly `bytes`, making its folder first: a new temp file beside it,
/// written, synced, then renamed over `target`, so a reader sees the old file or the new one and
/// never half of one. On any error the temp file is removed.
pub(crate) fn replace(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = target.parent().ok_or(io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(dir)?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp = Temp(Some(
        dir.join(format!(".{name}.{}-{n}.tmp", std::process::id())),
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp.path())?;
    file.write_all(bytes).and_then(|()| file.sync_all())?;
    drop(file);
    temp.rename_to(target)
}

/// Removes the file at `path`; a missing file is no error.
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// Removes the folder at `path` and everything in it; a missing folder is no error.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// A temp file, removed on drop unless it was renamed.
struct Temp(Option<PathBuf>);

impl Temp {
    fn path(&self) -> &Path {
        self.0.as_deref().unwrap_or_else(|| Path::new(""))
    }

    /// Renames the temp file over `target`.
    fn rename_to(mut self, target: &Path) -> io::Result<()> {
        let Some(temp) = self.0.take() else {
            return Ok(());
        };
        if let Err(err) = std::fs::rename(&temp, target) {
            self.0 = Some(temp);
            return Err(err);
        }
        Ok(())
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        if let Some(temp) = self.0.take() {
            // Best effort: a temp file left behind holds closed values only, and the next
            // `remove_dir_all` of the counts folder takes it.
            drop(std::fs::remove_file(&temp));
        }
    }
}

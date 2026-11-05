//! The Windows sandbox's record files (`<data_dir>\sandbox\matters.json`, `setup.json`): the
//! folder they live in and their atomic replacement. The one place those modules change files;
//! workspace `clippy.toml` bans each call, and the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the sandbox's record files, outside the jail, are written here alone"
)]

use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::Path;

/// Makes the folder `path` and every missing parent; a folder already there is fine.
pub(crate) fn make_dirs(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Replaces the file `path` with `bytes` in one step: the bytes go to a new file beside it
/// (`.<name>.<16 hex>.tmp`, made with `create_new`, so nothing there already is opened), are
/// flushed to disk, and the new file is renamed over `path` (`MoveFileExW` with
/// `MOVEFILE_REPLACE_EXISTING` on Windows). A reader sees the old file or the new one, never a
/// part. On failure the new file is removed and `path` is as it was.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let mut suffix = [0u8; 8];
    getrandom::fill(&mut suffix).map_err(|error| io::Error::other(error.to_string()))?;
    let hex = suffix.iter().fold(String::new(), |mut text, byte| {
        text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        text.push(char::from_digit(u32::from(byte & 0xf), 16).unwrap_or('0'));
        text
    });
    let temp = path.with_file_name(format!(".{}.{hex}.tmp", name.to_string_lossy()));
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if written.is_err() {
        // The new file may be missing already; nothing else to do either way.
        drop(std::fs::remove_file(&temp));
    }
    written
}

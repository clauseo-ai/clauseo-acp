//! The Windows sandbox's record files (`<data_dir>\sandbox\matters.json`, `setup.json`): the
//! folder they live in, its lock file, and their atomic replacement. The one place those modules
//! change files; workspace `clippy.toml` bans each call, and the expect below covers only this
//! file.

#![expect(
    clippy::disallowed_methods,
    reason = "the sandbox's record files, outside the jail, are written here alone"
)]

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write as _};
use std::path::Path;
use std::time::Duration;

/// Makes the sandbox folder `path`; one already there is fine. Its parents are made as
/// `create_dir_all` makes them. On Windows the folder itself is made with its protected DACL
/// already in place (`private_dir::create`: the desktop user, `SYSTEM` and `Administrators`
/// with full access, nothing inherited); a folder already there is checked and secured again
/// (`private_dir::secure_existing`: not a link, owner taken back or refused, the DACL set again,
/// explicit entries dropped from what is inside).
pub(crate) fn make_sandbox_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    #[cfg(windows)]
    if !super::private_dir::create(path)? {
        super::private_dir::secure_existing(path)?;
    }
    #[cfg(not(windows))]
    match std::fs::create_dir(path) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    Ok(())
}

/// How long [`lock_exclusive`] waits for another holder.
pub(crate) const LOCK_WAIT: Duration = Duration::from_secs(10);
/// How often a waiting lock tries again.
const LOCK_RETRY: Duration = Duration::from_millis(50);

/// Opens the lock file `path` (made if missing, never truncated) and takes its exclusive OS
/// lock (`LockFileEx` on Windows, `flock` elsewhere), waiting up to [`LOCK_WAIT`] for any other
/// holder, in this process or another. Dropping the file releases the lock.
pub(crate) fn lock_exclusive(path: &Path) -> io::Result<File> {
    lock_exclusive_for(path, LOCK_WAIT)
}

/// [`lock_exclusive`], waiting up to `wait`: `try_lock` every 50 ms. A lock still held after
/// `wait` is a `WouldBlock` error that names the lock file, not its path.
pub(crate) fn lock_exclusive_for(path: &Path, wait: Duration) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let tries = wait.as_millis() / LOCK_RETRY.as_millis() + 1;
    for attempt in 0..tries {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if attempt + 1 < tries => std::thread::sleep(LOCK_RETRY),
            Err(TryLockError::WouldBlock) => break,
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        format!("the lock {name} is still held by another Clauseo after {wait:?}"),
    ))
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

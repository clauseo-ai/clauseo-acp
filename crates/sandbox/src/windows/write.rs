//! The Windows sandbox's record files (`<data_dir>\sandbox\matters.json`, `setup.json`): the
//! folder they live in, its lock file, and their atomic replacement. The one place those modules
//! change files; workspace `clippy.toml` bans each call, and the expect below covers only this
//! file.

#![expect(
    clippy::disallowed_methods,
    reason = "the sandbox's record files, outside the jail, are written here alone"
)]

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

/// Makes the sandbox folder `path`; one already there is fine. Its parents are made as
/// `create_dir_all` makes them. On Windows the folder itself is made with its protected DACL
/// already in place (`private_dir::create`: the desktop user, `SYSTEM` and `Administrators`
/// with full access, nothing inherited); a folder already there is opened without following a
/// link and secured through that handle (`private_dir::protect`: refused if a link,
/// owner taken back or refused, the DACL set again). Files inside are checked one by one by the
/// code that uses them ([`check_owned_file`]); nothing walks the folder.
pub(crate) fn make_sandbox_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    #[cfg(windows)]
    if !super::private_dir::create(path)? {
        super::private_dir::protect(path)?;
    }
    #[cfg(not(windows))]
    match std::fs::create_dir(path) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    Ok(())
}

/// Renames `from` (not followed if a link) to `to`, refusing (`AlreadyExists`) when something
/// is at `to`, since a rename over a file (or, on Unix, an empty folder) would succeed. For the
/// set-aside's tests: the agent's renames, and the moves of their stand-in for NTFS.
#[cfg(test)]
pub(crate) fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(to) {
        Ok(_) => return Err(io::Error::from(io::ErrorKind::AlreadyExists)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::rename(from, to)
}

/// What the repair found on an opened record file or the sandbox folder, through its handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Found {
    /// The name is a reparse point (a link or a junction).
    pub(crate) reparse: bool,
    /// It is the kind expected: a folder for the sandbox folder, a file for a record.
    pub(crate) right_kind: bool,
    /// Its hard link count (1 for a folder).
    pub(crate) links: u32,
    /// Its owner is the user, `SYSTEM` or `Administrators`.
    pub(crate) owner_allowed: bool,
}

/// What the repair does with what it [`Found`]: refuse (why), or take ownership back for the
/// user first (`Ok(true)`) or not (`Ok(false)`), then set the DACL (the folder's protected
/// one, or the same entries on a file). Fails closed: a link, the wrong kind or a second hard
/// link (which would carry the DACL change to a file elsewhere) is refused.
pub(crate) fn repair_plan(found: Found) -> Result<bool, String> {
    if found.reparse {
        return Err("is a link (a reparse point)".to_owned());
    }
    if !found.right_kind {
        return Err("is not the kind of entry expected".to_owned());
    }
    if found.links > 1 {
        return Err(format!("has {} hard links", found.links));
    }
    Ok(!found.owner_allowed)
}

/// Checks a file this crate keeps in the sandbox folder (`matters.json`, `matters.lock`,
/// `setup.json`, the ACE ledger and its lock), before reading or writing it, under its lock
/// where it has one, and again after a save puts a new file in place. A missing file is fine.
/// On Windows the file is opened without following a link and, through that handle, refused
/// when [`repair_plan`] refuses, taken back for the user when another account owns it (or
/// refused when it can't be), and given the folder's three entries as its own protected DACL
/// (`private_dir::check_file`). Elsewhere, where only tests use it, a link or a second hard
/// link is refused.
pub(crate) fn check_owned_file(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        super::private_dir::check_file(path)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let found = Found {
            reparse: metadata.file_type().is_symlink(),
            right_kind: metadata.is_file(),
            links: u32::try_from(metadata.nlink()).unwrap_or(u32::MAX),
            owner_allowed: true,
        };
        repair_plan(found).map(drop).map_err(|why| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {why}", path.display()),
            )
        })
    }
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

/// Removes the file this crate keeps at `path` in the sandbox folder, checked first as
/// [`check_owned_file`] checks it, so a link there is refused, never followed. A missing file is
/// fine. The clean-close record goes this way at every launch (S15 `PR3d`).
#[cfg(any(windows, test))]
pub(crate) fn remove_owned_file(path: &Path) -> io::Result<()> {
    check_owned_file(path)?;
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Replaces the file `path` with `bytes` in one step: the bytes go to a new file beside it
/// (`.<name>.<16 hex>.tmp`, made with `create_new`, so nothing there already is opened), are
/// flushed to disk, and the new file is renamed over `path` (`MoveFileExW` with
/// `MOVEFILE_REPLACE_EXISTING` on Windows). A reader sees the old file or the new one, never a
/// part. On failure the new file is removed and `path` is as it was.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_with(path, bytes, Write::write_all)
}

/// [`write_atomic`] with the write itself as a parameter, so a test can fail it partway.
pub(crate) fn write_atomic_with(
    path: &Path,
    bytes: &[u8],
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
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
        write(&mut file, bytes)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: Found = Found {
        reparse: false,
        right_kind: true,
        links: 1,
        owner_allowed: true,
    };

    #[test]
    fn the_repair_refuses_links_kinds_and_hard_links_and_reowns_others() {
        assert_eq!(repair_plan(PLAIN), Ok(false));
        let foreign = Found {
            owner_allowed: false,
            ..PLAIN
        };
        assert_eq!(repair_plan(foreign), Ok(true));
        for refused in [
            Found {
                reparse: true,
                ..foreign
            },
            Found {
                right_kind: false,
                ..PLAIN
            },
            Found { links: 2, ..PLAIN },
        ] {
            assert!(repair_plan(refused).is_err(), "{refused:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_file_is_fine_and_a_link_or_hard_link_is_refused() {
        use crate::test_support::TempDir;

        let temp = TempDir::new("check-owned-file");
        assert!(check_owned_file(&temp.path().join("matters.json")).is_ok());
        let file = temp.file("matters.json", "{}");
        assert!(check_owned_file(&file).is_ok());
        let linked = temp.hard_link("other.json", &file);
        assert!(check_owned_file(&file).is_err());
        assert!(check_owned_file(&linked).is_err());
        let target = temp.file("target.json", "{}");
        let symlink = temp.symlink("link.json", &target);
        assert!(check_owned_file(&symlink).is_err());
    }
}

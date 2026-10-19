//! The staging dirs `setup --binaries` downloads into, inside cargo's bin: each is locked by the
//! run that uses it, and the sweep at the start of a run removes only those whose lock is free.

use super::Machine;
use crate::output::Printer;
use std::fs::{self, File, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The start of the name of every staging directory `setup` makes in bin.
const STAGING_PREFIX: &str = ".xtask-setup-";

/// The file in a staging dir that the run using the dir holds an exclusive lock on.
const LOCK_FILE: &str = "lock";

/// The start of the name a staging dir has while it is made, before its lock is held.
const PENDING_PREFIX: &str = ".xtask-pending-";

/// Makes `bin` if needed, and in it an empty staging dir for `name` (a binary and this run's
/// pid), and returns that dir with its lock file, whose lock it holds. The dir is made and locked
/// under a `PENDING_PREFIX` name the sweep does not match, then renamed to its `STAGING_PREFIX`
/// name, so a sweep never sees it unlocked. A run killed between the two leaves a pending dir; a
/// later run with the same pid removes it.
pub(super) fn fresh_dir(bin: &Path, name: &str) -> io::Result<(PathBuf, File)> {
    let staging = bin.join(format!("{STAGING_PREFIX}{name}"));
    let pending = bin.join(format!("{PENDING_PREFIX}{name}"));
    fs::create_dir_all(bin)?;
    remove_dir_if_there(&pending)?;
    remove_dir_if_there(&staging)?;
    fs::create_dir(&pending)?;
    let locked = File::create(pending.join(LOCK_FILE)).and_then(|lock| {
        lock.try_lock().map_err(io::Error::from)?;
        fs::rename(&pending, &staging)?;
        Ok(lock)
    });
    if locked.is_err() {
        // The error being returned says what went wrong; this only tidies up after it.
        drop(remove_dir_if_there(&pending));
    }
    Ok((staging, locked?))
}

/// Removes `dir` and what it holds; a `dir` that is not there is fine.
fn remove_dir_if_there(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Removes the staging leftovers in `machine`'s cargo bin, on macOS and Linux, printing a failure
/// when that fails. Returns whether it worked. `setup --binaries` runs it first, whether or not
/// anything downloads. Only `setup` makes `STAGING_PREFIX` entries. A run holds the lock on its
/// staging dir's lock file while it uses the dir, and the OS drops the lock when the run ends, so
/// a dir whose lock is free is from a run that was interrupted (Ctrl-C, a kill) before it removed
/// its own; one whose lock is held is in use by another setup and is left alone.
pub(crate) fn clear_staging_leftovers<W: Write>(
    out: &mut Printer<W>,
    machine: &Machine<'_>,
) -> bool {
    let Some(bin) = machine
        .cargo_bin
        .as_deref()
        .filter(|_| machine.os != "windows")
    else {
        return true;
    };
    match remove_staging_leftovers(bin) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => {
            out.failure(
                &format!(
                    "could not remove what an interrupted setup left in {}: {error}",
                    bin.display()
                ),
                &format!(
                    "once no other setup is running, remove the {STAGING_PREFIX}* entries in {} \
                     by hand",
                    bin.display()
                ),
            );
            false
        }
    }
}

/// Removes every entry in `bin` whose name starts with `STAGING_PREFIX`, except a dir whose lock
/// file another run holds the lock on. A symlink is removed, never followed.
fn remove_staging_leftovers(bin: &Path) -> io::Result<()> {
    for entry in fs::read_dir(bin)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            continue;
        }
        let path = entry.path();
        if !fs::symlink_metadata(&path)?.is_dir() {
            fs::remove_file(&path)?;
            continue;
        }
        // Held through the removal, so no run can take the dir over meanwhile.
        let _lock = match File::open(path.join(LOCK_FILE)) {
            Ok(lock) => match lock.try_lock() {
                Ok(()) => Some(lock),
                Err(TryLockError::WouldBlock) => continue,
                Err(TryLockError::Error(error)) => return Err(error),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        fs::remove_dir_all(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

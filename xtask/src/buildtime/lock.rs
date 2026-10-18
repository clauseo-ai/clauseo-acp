//! One `buildtime rust` run per checkout. Two runs would take each other's saved copies for
//! leftovers, so a run holds an exclusive lock on `target/xtask/buildtime.lock` from before the
//! leftover restore to its end. The OS drops the lock when the process ends, killed or not, so a
//! lock is never stale and nothing cleans it up.

use super::verdict::Failure;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;

/// The lock file, under the repo root.
pub(crate) const LOCK_PATH: &str = "target/xtask/buildtime.lock";

/// Takes the exclusive lock on the file at `path`, creating it when it is missing. The lock holds
/// as long as the returned `File` is alive. Fails at once when another run holds it.
pub(crate) fn acquire(path: &Path) -> Result<File, Failure> {
    let opened = match path.parent() {
        Some(dir) => fs::create_dir_all(dir),
        None => Ok(()),
    }
    .and_then(|()| {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
    });
    let file = opened.map_err(|error| {
        Failure::new(
            format!("could not open {LOCK_PATH}: {error}"),
            "make sure target/xtask is writable, then run again",
        )
    })?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(Failure::new(
            "another buildtime run is in progress in this checkout",
            "wait for it to finish, or stop it",
        )),
        Err(TryLockError::Error(error)) => Err(Failure::new(
            format!("could not lock {LOCK_PATH}: {error}"),
            format!("make sure {LOCK_PATH} can be locked, then run again"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_fails_while_the_first_is_held_and_works_after() {
        let dir = std::env::temp_dir().join(format!("xtask-buildtime-lock-{}", std::process::id()));
        let path = dir.join("target/xtask/buildtime.lock");
        let first = acquire(&path).expect("the first run takes the lock");
        let second = acquire(&path).expect_err("the second run is refused");
        assert_eq!(
            second.message,
            "another buildtime run is in progress in this checkout"
        );
        assert_eq!(second.fix, "wait for it to finish, or stop it");
        drop(first);
        let third = acquire(&path).expect("the lock is free once the first run ends");
        drop(third);
        fs::remove_dir_all(&dir).expect("remove the scratch dir");
    }
}

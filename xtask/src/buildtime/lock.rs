//! One `buildtime` run per checkout. Two runs would take each other's saved copies for leftovers,
//! or edit `ui/app/src/App.tsx` at once, so a run holds an exclusive lock on
//! `target/xtask/buildtime.lock` from before the leftover restore to the end of its last half.
//! The OS drops the lock when the process ends, killed or not, so a lock is never stale and nothing
//! cleans it up.

use super::verdict::Failure;
use crate::write;
use std::fs::{File, TryLockError};
use std::path::Path;

/// The lock file, under the repo root.
pub(crate) const LOCK_PATH: &str = "target/xtask/buildtime.lock";

/// Takes the exclusive lock on the file at `path`, creating it when it is missing. The lock holds
/// as long as the returned `File` is alive. Fails at once when another run holds it.
pub(crate) fn acquire(path: &Path) -> Result<File, Failure> {
    let file = write::create_lock_file(path).map_err(|error| {
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

/// True when another open handle holds the lock on the file at `path`. A free lock is taken and
/// released at once. A missing file is never created and reads as free, as does any error.
pub(crate) fn is_held(path: &Path) -> bool {
    let Ok(file) = write::open_lock_file(path) else {
        return false;
    };
    matches!(file.try_lock(), Err(TryLockError::WouldBlock))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_second_lock_fails_while_the_first_is_held_and_works_after() {
        let dir = Scratch::new("buildtime-lock");
        let path = dir.join("target/xtask/buildtime.lock");
        let first = acquire(&path).expect("the first run takes the lock");
        let second = acquire(&path).expect_err("the second run is refused");
        assert_eq!(
            second.message,
            "another buildtime run is in progress in this checkout"
        );
        assert_eq!(second.fix, "wait for it to finish, or stop it");
        assert!(is_held(&path));
        drop(first);
        assert!(!is_held(&path));
        let third = acquire(&path).expect("the lock is free once the first run ends");
        drop(third);
    }
}

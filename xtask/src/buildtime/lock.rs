//! One `buildtime` run per checkout. Two runs would take each other's saved copies for leftovers,
//! or edit `ui/app/src/App.tsx` at once, so a run holds an exclusive lock on
//! `target/xtask/buildtime.lock` from before the leftover restore to the end of its last half.
//! The OS drops the lock when the process ends, killed or not, so a lock is never stale and nothing
//! cleans it up.

use super::verdict::Failure;
use crate::write;
use crate::write::LockFile;
use std::fs::TryLockError;
use std::path::Path;

/// The lock file's name, in `write::LOCK_DIR`.
const LOCK_NAME: &str = "buildtime.lock";

/// The lock file, under the repo root.
pub(crate) const LOCK_PATH: &str = "target/xtask/buildtime.lock";

/// Takes the exclusive lock on `LOCK_PATH` under `root`, creating the file when it is missing.
/// The lock holds as long as the returned `LockFile` is alive. Fails at once when another run
/// holds it.
pub(crate) fn acquire(root: &Path) -> Result<LockFile, Failure> {
    let file = write::create_lock_file(root, LOCK_NAME).map_err(|error| {
        Failure::new(
            format!("could not open {LOCK_PATH}: {error}"),
            "fix what the error names, then run again",
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

/// True when another open handle holds the lock on `LOCK_PATH` under `root`. A free lock is taken
/// and released at once. A missing file is never created and reads as free, as does any error.
pub(crate) fn is_held(root: &Path) -> bool {
    let Ok(file) = write::open_lock_file(root, LOCK_NAME) else {
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
        assert_eq!(LOCK_PATH, format!("{}/{LOCK_NAME}", write::LOCK_DIR));
        let first = acquire(&dir).expect("the first run takes the lock");
        let second = acquire(&dir).expect_err("the second run is refused");
        assert_eq!(
            second.message,
            "another buildtime run is in progress in this checkout"
        );
        assert_eq!(second.fix, "wait for it to finish, or stop it");
        assert!(is_held(&dir));
        drop(first);
        assert!(!is_held(&dir));
        let third = acquire(&dir).expect("the lock is free once the first run ends");
        drop(third);
    }
}

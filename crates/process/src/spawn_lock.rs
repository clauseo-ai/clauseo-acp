//! The process-wide spawn lock: one lock every start in this process holds while it creates the
//! child, so a start that marks handles inheritable for itself never races another start.
//!
//! On Windows, `CreateProcess*` with `bInheritHandles` passes every inheritable handle of the
//! process unless the call lists them (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`); std's `Command`
//! does not list them. A start that turns `HANDLE_FLAG_INHERIT` on for its own child's handles
//! (`clauseo-lowbox`'s launch, an owner passing a namespace) holds this lock from the flag's
//! set to its reset, and [`spawn_in_tree`](crate::spawn_in_tree) holds it across its spawn, so
//! neither child gets the other's handles. A start in this process that takes neither path
//! (a bare `std::process::Command`) is not covered; such callers take the lock themselves.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// The lock. It guards no data: holding it is the point.
static SPAWN: Mutex<()> = Mutex::new(());

/// Holds the process-wide spawn lock until dropped. Not reentrant: a start taken while this
/// thread holds it (`spawn_in_tree`, `clauseo-lowbox`'s spawn) waits forever, so drop it first.
#[derive(Debug)]
#[must_use = "the lock is released when this is dropped"]
pub struct SpawnLock {
    /// The guard of [`SPAWN`].
    _guard: MutexGuard<'static, ()>,
}

/// Takes the process-wide spawn lock, waiting for a start in progress on another thread. A
/// thread that panicked while holding it leaves nothing to repair (the lock guards no data),
/// so poisoning is ignored.
pub fn spawn_lock() -> SpawnLock {
    SpawnLock {
        _guard: SPAWN.lock().unwrap_or_else(PoisonError::into_inner),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::TryLockError;
    use std::thread;

    use super::{SPAWN, spawn_lock};

    /// Whether another thread finds the lock taken. A poisoned lock (the panic test, when the
    /// tests share a process) counts as free: `spawn_lock` ignores the poison.
    fn taken_elsewhere() -> bool {
        thread::spawn(|| matches!(SPAWN.try_lock(), Err(TryLockError::WouldBlock)))
            .join()
            .unwrap()
    }

    #[test]
    fn the_lock_is_taken_until_dropped() {
        let held = spawn_lock();
        assert!(
            taken_elsewhere(),
            "another thread took the lock while it was held"
        );
        drop(held);
        assert!(!taken_elsewhere());
    }

    #[test]
    fn a_panic_while_held_does_not_poison_it() {
        let panicked = thread::spawn(|| {
            let _lock = spawn_lock();
            panic!("a start failed while holding the lock");
        })
        .join();
        assert!(panicked.is_err());
        drop(spawn_lock());
    }
}

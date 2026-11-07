//! An answer a thread of its own works out while the caller goes on ([`Pending`]): the session's
//! drives (`listing::DRIVES_WAIT`) and the pre-check (`stall::PROBE_WAIT`), so neither delays the
//! chooser's window. The work is bounded itself; a waiter gives up a little after that bound.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// How long past the work's own bound a waiter still waits for the thread to hand its answer in.
pub(crate) const HAND_IN: Duration = Duration::from_millis(250);

/// An answer on its way from a thread, or landed.
pub(crate) struct Pending<T> {
    slot: Mutex<Option<T>>,
    landed: Condvar,
}

impl<T: Clone + Send + 'static> Pending<T> {
    /// Starts `work` on a thread named `name`; `fallback` is the answer when no thread starts.
    /// `work` must end within its own bound: nothing here stops it.
    pub(crate) fn start(
        name: &str,
        fallback: T,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Arc<Self> {
        let pending = Arc::new(Self {
            slot: Mutex::new(None),
            landed: Condvar::new(),
        });
        let filler = Arc::clone(&pending);
        let spawned = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || filler.land(work()));
        if spawned.is_err() {
            pending.land(fallback);
        }
        pending
    }

    /// An answer already landed.
    pub(crate) fn landed(answer: T) -> Arc<Self> {
        Arc::new(Self {
            slot: Mutex::new(Some(answer)),
            landed: Condvar::new(),
        })
    }

    fn land(&self, answer: T) {
        *lock(&self.slot) = Some(answer);
        self.landed.notify_all();
    }

    /// The answer, if it has landed, without waiting.
    pub(crate) fn now(&self) -> Option<T> {
        lock(&self.slot).clone()
    }

    /// The answer, waiting at most `wait` for it; `None` when it has not landed by then.
    pub(crate) fn get(&self, wait: Duration) -> Option<T> {
        let guard = lock(&self.slot);
        let (guard, _timeout) = self
            .landed
            .wait_timeout_while(guard, wait, |slot| slot.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        guard.clone()
    }
}

/// Locks `mutex`, past a poisoned lock.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

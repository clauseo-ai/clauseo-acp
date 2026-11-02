//! [`CancelFlag`]: a flag another thread sets, and the one way this crate waits.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// A flag that, once set, stays set. Clones share it. Waiting on it is how the crate sleeps:
/// [`CancelFlag::wait_timeout`] returns as soon as the flag is set.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag {
    inner: Arc<(Mutex<bool>, Condvar)>,
}

impl CancelFlag {
    /// A flag that is not set.
    pub fn new() -> CancelFlag {
        CancelFlag::default()
    }

    /// Sets the flag and wakes every waiter.
    pub fn cancel(&self) {
        let (lock, wake) = &*self.inner;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = true;
        wake.notify_all();
    }

    /// Whether the flag is set.
    pub fn is_cancelled(&self) -> bool {
        *self.inner.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits up to `timeout` of real time, or until the flag is set. Returns whether it is set.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let (lock, wake) = &*self.inner;
        let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = wake
            .wait_timeout_while(guard, timeout, |cancelled| !*cancelled)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wait_times_out_when_nobody_cancels() {
        let flag = CancelFlag::new();
        assert!(!flag.wait_timeout(Duration::from_millis(10)));
        assert!(!flag.is_cancelled());
    }

    #[test]
    fn cancel_wakes_a_waiter_on_another_thread() {
        let flag = CancelFlag::new();
        let waiter = flag.clone();
        let thread = std::thread::spawn(move || waiter.wait_timeout(Duration::from_secs(30)));
        flag.cancel();
        assert!(thread.join().unwrap());
        assert!(flag.is_cancelled());
        assert!(
            flag.wait_timeout(Duration::from_secs(30)),
            "a set flag returns at once"
        );
    }
}

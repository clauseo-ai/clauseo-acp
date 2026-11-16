//! One value per process, opened by its first taker and dropped when its last taker lets go,
//! with the open and the drop both under one lock. The set-up lock keeps its private namespace
//! here (`lock.rs`): a process may have a given private namespace open only once, so a second
//! open while the first handle lives fails (`ERROR_DUP_NAME`), and so does one made after the
//! last taker let go but before the handle was closed. Closing under the same lock as opening
//! rules both out. Plain Rust with no Windows calls, so its tests run on every OS.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// The value and how many [`Claim`]s hold it. `value` is `Some` exactly while `holders > 0`.
#[derive(Debug)]
pub(super) struct Shared<T> {
    state: Mutex<State<T>>,
}

#[derive(Debug)]
struct State<T> {
    value: Option<T>,
    holders: usize,
}

impl<T> Shared<T> {
    /// Nothing open, no holders; for a `static`.
    pub(super) const fn new() -> Shared<T> {
        Shared {
            state: Mutex::new(State {
                value: None,
                holders: 0,
            }),
        }
    }

    /// A claim on the value, opened with `open` when no claim holds it. `open` runs under the
    /// lock, so two first takers never both open; a failed open leaves nothing held.
    pub(super) fn claim<E>(
        &'static self,
        open: impl FnOnce() -> Result<T, E>,
        overflow: impl FnOnce() -> E,
    ) -> Result<Claim<T>, E> {
        let mut state = self.lock();
        let Some(holders) = state.holders.checked_add(1) else {
            return Err(overflow());
        };
        if state.value.is_none() {
            state.value = Some(open()?);
        }
        state.holders = holders;
        Ok(Claim { shared: self })
    }

    /// The state, recovered when a panic poisoned the lock: every change to it is made after
    /// the step that could fail, so a poisoned state is still a consistent one.
    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One taker's hold on a [`Shared`] value. The last one dropped drops the value, under the lock.
#[derive(Debug)]
pub(super) struct Claim<T: 'static> {
    shared: &'static Shared<T>,
}

impl<T> Drop for Claim<T> {
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.holders = state.holders.saturating_sub(1);
        if state.holders == 0 {
            // Dropped while the guard is held: no taker can open a new value before this one
            // is gone.
            drop(state.value.take());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use super::Shared;

    /// A value that refuses to open while another one lives, as a private namespace does.
    #[derive(Debug)]
    struct Once(&'static AtomicUsize);

    impl Once {
        fn open(live: &'static AtomicUsize, opens: &AtomicUsize) -> Result<Once, &'static str> {
            opens.fetch_add(1, Ordering::SeqCst);
            if live.fetch_add(1, Ordering::SeqCst) != 0 {
                live.fetch_sub(1, Ordering::SeqCst);
                return Err("duplicate name");
            }
            Ok(Once(live))
        }
    }

    impl Drop for Once {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn takers_while_one_holds_share_one_open_and_the_last_drop_closes_it() {
        static SHARED: Shared<Once> = Shared::new();
        static LIVE: AtomicUsize = AtomicUsize::new(0);
        let opens = AtomicUsize::new(0);
        let open = || Once::open(&LIVE, &opens);
        let first = SHARED.claim(open, || "overflow").unwrap();
        let second = SHARED.claim(open, || "overflow").unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 1);
        drop(first);
        assert_eq!(LIVE.load(Ordering::SeqCst), 1);
        drop(second);
        assert_eq!(LIVE.load(Ordering::SeqCst), 0);
        let _third = SHARED.claim(open, || "overflow").unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_failed_open_leaves_nothing_held() {
        static SHARED: Shared<Once> = Shared::new();
        static LIVE: AtomicUsize = AtomicUsize::new(0);
        let failed = SHARED.claim(|| Err::<Once, _>("refused"), || "overflow");
        assert!(matches!(failed, Err("refused")));
        let opens = AtomicUsize::new(0);
        let _claim = SHARED
            .claim(|| Once::open(&LIVE, &opens), || "overflow")
            .unwrap();
        assert_eq!(opens.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn two_threads_claiming_and_dropping_never_open_twice() {
        static SHARED: Shared<Once> = Shared::new();
        static LIVE: AtomicUsize = AtomicUsize::new(0);
        static OPENS: AtomicUsize = AtomicUsize::new(0);
        let takers: Vec<_> = (0..2)
            .map(|_| {
                thread::spawn(|| {
                    for _round in 0..2000 {
                        let claim = SHARED.claim(|| Once::open(&LIVE, &OPENS), || "overflow");
                        drop(claim.unwrap());
                    }
                })
            })
            .collect();
        for taker in takers {
            taker.join().unwrap();
        }
        assert_eq!(LIVE.load(Ordering::SeqCst), 0);
    }
}

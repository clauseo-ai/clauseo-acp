use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;

/// A platform that records what it lowers and restores: each lowered thread is a number.
#[derive(Default)]
struct Fake {
    next: AtomicUsize,
    restored: Arc<Mutex<Vec<usize>>>,
}

impl Priority for Fake {
    type Thread = usize;
    fn lower_current(&self) -> Option<usize> {
        Some(self.next.fetch_add(1, Ordering::Relaxed))
    }
    fn restore(&self, thread: usize) {
        self.restored.lock().unwrap().push(thread);
    }
}

fn lowering() -> (Lowering<Fake>, Arc<Mutex<Vec<usize>>>) {
    let fake = Fake::default();
    let restored = Arc::clone(&fake.restored);
    (Lowering::new(fake), restored)
}

/// The host-start thread's shape: lowered, guarded, then the start's outcome.
fn start(lowering: &Lowering<Fake>, outcome: Result<(), &str>) -> Result<(), String> {
    let _restore = lowering.restore_on_drop();
    lowering.lower_current("clauseo-host-start");
    lowering.lower_current("clauseo-setup");
    outcome.map_err(str::to_owned)
}

#[test]
fn a_start_that_ends_restores_every_lowered_thread() {
    let (lowering, restored) = lowering();
    assert_eq!(start(&lowering, Ok(())), Ok(()));
    assert_eq!(*restored.lock().unwrap(), [0, 1]);
}

#[test]
fn a_start_that_fails_still_restores() {
    let (lowering, restored) = lowering();
    assert!(start(&lowering, Err("store refused")).is_err());
    assert_eq!(*restored.lock().unwrap(), [0, 1]);
}

#[test]
fn a_start_that_panics_still_restores() {
    let (lowering, restored) = lowering();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _restore = lowering.restore_on_drop();
        lowering.lower_current("clauseo-host-start");
        panic!("the start panicked");
    }));
    assert!(panicked.is_err());
    assert_eq!(*restored.lock().unwrap(), [0]);
}

#[test]
fn after_the_restore_no_thread_is_lowered_and_nothing_is_restored_twice() {
    let (lowering, restored) = lowering();
    assert!(lowering.lower_current("a"));
    assert_eq!(lowering.restore_all(), 1);
    assert!(!lowering.lower_current("late"));
    assert_eq!(lowering.restore_all(), 0);
    assert_eq!(*restored.lock().unwrap(), [0]);
}

#[cfg(not(windows))]
#[test]
fn off_windows_the_os_lowers_nothing() {
    assert!(Os.lower_current().is_none());
}

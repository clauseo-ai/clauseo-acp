//! The set-up lock on Windows: exclusive across threads, given back on drop, and taken when
//! abandoned. These run in CI's ordinary test step (not the `elevated_` one): the hosted runner's
//! token is elevated (split §8). On an unelevated token they skip, since only an elevated token
//! may make an object in `Global\` or open one with the lock's DACL.

use std::sync::mpsc;
use std::thread;

use super::lock::{Acquired, SetupMutex};
use super::machine::{Machine, MachineError};
use super::windows_machine::{Names, WindowsMachine};
use crate::windows::test_dacl::elevated;

/// A mutex name in `Global\` no other test or run uses; `None` (the test skips) on an
/// unelevated token.
pub(super) fn unique_mutex() -> Option<String> {
    if !elevated() {
        eprintln!("skipped: the set-up lock needs an elevated token");
        return None;
    }
    Some(format!("Global\\ClauseoSandboxSetupTest-{}", suffix()))
}

/// A short random suffix: six hex digits from the OS's random source.
pub(super) fn suffix() -> String {
    let mut bytes = [0u8; 3];
    getrandom::fill(&mut bytes).unwrap();
    let [a, b, c] = bytes;
    format!("{a:02x}{b:02x}{c:02x}")
}

/// A machine whose only name in use is the mutex `mutex`.
pub(super) fn machine(mutex: &str) -> WindowsMachine {
    let names = Names {
        mutex: mutex.to_owned(),
        ..Names::production()
    };
    WindowsMachine::new(names, Box::new(super::accounts_tests::NoSurroundings))
}

#[test]
fn the_lock_is_exclusive_across_two_threads_and_given_back_on_drop() {
    let Some(name) = unique_mutex() else {
        return;
    };
    let (taken_tx, taken_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let holder = {
        let name = name.clone();
        thread::spawn(move || {
            let (held, acquired) = SetupMutex::open(&name).unwrap().acquire(0).unwrap();
            assert_eq!(acquired, Acquired::Free);
            taken_tx.send(()).unwrap();
            done_rx.recv().unwrap();
            drop(held);
        })
    };
    taken_rx.recv().unwrap();
    let refused = SetupMutex::open(&name).unwrap().acquire(0);
    assert!(
        matches!(refused, Err(MachineError::LockHeld)),
        "{refused:?}"
    );
    // A short wait is refused too while the holder keeps it.
    let refused = SetupMutex::open(&name).unwrap().acquire(50);
    assert!(
        matches!(refused, Err(MachineError::LockHeld)),
        "{refused:?}"
    );
    done_tx.send(()).unwrap();
    holder.join().unwrap();
    let (_held, acquired) = SetupMutex::open(&name).unwrap().acquire(0).unwrap();
    assert_eq!(acquired, Acquired::Free);
}

#[test]
fn a_lock_whose_thread_ended_holding_it_counts_as_acquired() {
    // Release builds abort on panic, so a crashed set-up never releases: Windows abandons the
    // mutex when the owning thread ends. This handle keeps the mutex alive meanwhile.
    let Some(name) = unique_mutex() else {
        return;
    };
    let waiting = SetupMutex::open(&name).unwrap();
    let crashed = {
        let name = name.clone();
        thread::spawn(move || {
            let (held, _) = SetupMutex::open(&name).unwrap().acquire(0).unwrap();
            held.abandon();
        })
    };
    crashed.join().unwrap();
    let (held, acquired) = waiting.acquire(0).unwrap();
    assert_eq!(acquired, Acquired::Abandoned);
    drop(held);
    // Once released properly, the next taker finds it free.
    let (_held, acquired) = SetupMutex::open(&name).unwrap().acquire(0).unwrap();
    assert_eq!(acquired, Acquired::Free);
}

#[test]
fn the_machines_lock_refuses_a_second_set_up_until_unlocked() {
    let Some(name) = unique_mutex() else {
        return;
    };
    let mut first = machine(&name);
    first.lock().unwrap();
    assert!(matches!(first.lock(), Err(MachineError::LockHeld)));
    let other = name.clone();
    let tried = thread::spawn(move || machine(&other).lock())
        .join()
        .unwrap();
    assert!(matches!(tried, Err(MachineError::LockHeld)), "{tried:?}");
    first.unlock();
    let other = name;
    let tried = thread::spawn(move || {
        let mut second = machine(&other);
        let locked = second.lock();
        second.unlock();
        locked
    })
    .join()
    .unwrap();
    assert!(tried.is_ok(), "{tried:?}");
}

#[test]
fn the_real_lock_name_is_in_the_global_namespace() {
    assert_eq!(Names::production().mutex, "Global\\ClauseoSandboxSetup");
    assert!(Names::production().mutex.starts_with("Global\\"));
}

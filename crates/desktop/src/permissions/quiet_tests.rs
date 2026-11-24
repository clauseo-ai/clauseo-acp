//! The keychain dialogs' switch (`quiet.rs`), over a fake OS switch that counts its calls.
//! Each test has its own switch: the counts are per fake type, so each test names its own.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;

use super::quiet::{Quiet, Switch};

/// A fake switch per test: its calls counted, its state kept, as the OS's would be.
macro_rules! fake_switch {
    ($name:ident) => {
        struct $name;

        impl $name {
            fn on() -> &'static AtomicBool {
                static ON: AtomicBool = AtomicBool::new(true);
                &ON
            }
            fn disables() -> &'static AtomicUsize {
                static DISABLES: AtomicUsize = AtomicUsize::new(0);
                &DISABLES
            }
        }

        /// Turns the dialogs on when dropped, as security-framework's lock does.
        struct Lock;

        impl Drop for Lock {
            fn drop(&mut self) {
                $name::on().store(true, Ordering::SeqCst);
            }
        }

        impl Switch for $name {
            type Lock = Lock;
            const REAL: bool = true;
            fn disable() -> Result<Lock, i32> {
                Self::disables().fetch_add(1, Ordering::SeqCst);
                Self::on().store(false, Ordering::SeqCst);
                Ok(Lock)
            }
            fn allowed() -> Option<bool> {
                Some(Self::on().load(Ordering::SeqCst))
            }
        }
    };
}

#[test]
fn starting_turns_the_dialogs_off_once() {
    fake_switch!(Os);
    let quiet = Quiet::<Os>::new();
    assert_eq!(Os::allowed(), Some(true));
    quiet.start();
    quiet.start();
    assert_eq!(Os::allowed(), Some(false));
    assert!(quiet.is_quiet());
    assert_eq!(Os::disables().load(Ordering::SeqCst), 1);
}

#[test]
fn an_ask_turns_the_dialogs_on_for_its_work_and_off_after() {
    fake_switch!(Os);
    let quiet = Quiet::<Os>::new();
    quiet.start();
    let during = quiet.with_dialogs_allowed(Os::allowed);
    assert_eq!(during, Some(true));
    assert_eq!(Os::allowed(), Some(false));
    assert!(quiet.is_quiet());
    assert_eq!(Os::disables().load(Ordering::SeqCst), 2);
}

#[test]
fn an_ask_that_panics_still_turns_the_dialogs_off() {
    fake_switch!(Os);
    let quiet = Quiet::<Os>::new();
    quiet.start();
    let ended = catch_unwind(AssertUnwindSafe(|| {
        quiet.with_dialogs_allowed(|| {
            assert_eq!(Os::allowed(), Some(true));
            panic!("a read panicked");
        })
    }));
    assert!(ended.is_err());
    assert_eq!(Os::allowed(), Some(false));
    assert!(quiet.is_quiet());
    // The next Ask still works: the turn was let go.
    assert_eq!(quiet.with_dialogs_allowed(Os::allowed), Some(true));
    assert_eq!(Os::allowed(), Some(false));
}

#[test]
fn an_ask_before_the_start_leaves_the_dialogs_off() {
    fake_switch!(Os);
    let quiet = Quiet::<Os>::new();
    quiet.with_dialogs_allowed(|| {});
    assert_eq!(Os::allowed(), Some(false));
}

#[test]
fn an_ask_inside_an_ask_runs_as_part_of_it() {
    fake_switch!(Os);
    let quiet = Quiet::<Os>::new();
    quiet.start();
    let inner = quiet.with_dialogs_allowed(|| {
        let inner = quiet.with_dialogs_allowed(Os::allowed);
        // Still on: the inner call did not end the outer one's window.
        assert_eq!(Os::allowed(), Some(true));
        // A silent check inside the window runs too, rather than waiting on itself.
        quiet.while_quiet(|| ());
        inner
    });
    assert_eq!(inner, Some(true));
    assert_eq!(Os::allowed(), Some(false));
    assert_eq!(Os::disables().load(Ordering::SeqCst), 2);
}

/// Gives other threads many chances to run, without a sleep: a thread that could go on would.
fn let_others_run() {
    for _ in 0..10_000 {
        std::thread::yield_now();
    }
}

#[test]
fn asks_on_two_threads_take_turns() {
    fake_switch!(Os);
    let quiet = Arc::new(Quiet::<Os>::new());
    quiet.start();
    let (opened, is_open) = channel();
    let (release, released) = channel::<()>();
    let first = {
        let quiet = Arc::clone(&quiet);
        std::thread::spawn(move || {
            quiet.with_dialogs_allowed(|| {
                opened.send(()).unwrap();
                released.recv().unwrap();
            });
        })
    };
    is_open.recv().unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let second = {
        let (quiet, entered) = (Arc::clone(&quiet), Arc::clone(&entered));
        std::thread::spawn(move || {
            quiet.with_dialogs_allowed(|| {
                entered.store(true, Ordering::SeqCst);
                assert_eq!(Os::allowed(), Some(true));
            });
        })
    };
    let_others_run();
    assert!(!entered.load(Ordering::SeqCst), "one Ask at a time");
    release.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert!(entered.load(Ordering::SeqCst));
    assert_eq!(Os::allowed(), Some(false));
}

#[test]
fn a_silent_check_waits_for_an_ask_s_window_to_close() {
    fake_switch!(Os);
    let quiet = Arc::new(Quiet::<Os>::new());
    quiet.start();
    let (opened, is_open) = channel();
    let (release, released) = channel::<()>();
    let ask = {
        let quiet = Arc::clone(&quiet);
        std::thread::spawn(move || {
            quiet.with_dialogs_allowed(|| {
                opened.send(()).unwrap();
                released.recv().unwrap();
            });
        })
    };
    is_open.recv().unwrap();
    let check = {
        let quiet = Arc::clone(&quiet);
        std::thread::spawn(move || quiet.while_quiet(Os::allowed))
    };
    let_others_run();
    assert!(
        !check.is_finished(),
        "the check waits while the window is open"
    );
    release.send(()).unwrap();
    ask.join().unwrap();
    // It ran only once the window closed: the dialogs were off again by then.
    assert_eq!(check.join().unwrap(), Some(false));
}

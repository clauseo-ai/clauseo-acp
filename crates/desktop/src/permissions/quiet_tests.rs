//! The keychain dialogs' switch (`quiet.rs`), over a fake OS switch that counts its calls.
//! Each test has its own switch: the counts are per fake type, so each test names its own.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
fn the_dialogs_stay_off_for_the_process_s_life() {
    fake_switch!(Os);
    let quiet = Quiet::<Os>::new();
    quiet.start();
    // Nothing in this process turns them on again: the Ask reads in a child (`ask_child.rs`).
    for _ in 0..3 {
        quiet.start();
    }
    assert_eq!(Os::allowed(), Some(false));
    assert_eq!(Os::disables().load(Ordering::SeqCst), 1);
}

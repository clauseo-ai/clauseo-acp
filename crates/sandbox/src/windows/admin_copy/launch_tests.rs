//! An elevated start through [`launch_checked`]: the copy is hashed through a held handle,
//! started while the handle is held, released after; a check that fails starts nothing. And
//! owners that can't be read fail closed.

use std::path::Path;

use super::fake::{Fake, HASH};
use super::*;
use crate::SetupState;

/// `launch_checked` on `fake`, its start saying `started` in the fake's events.
fn launch(fake: &Fake) -> Result<(), Blocked> {
    let events = std::rc::Rc::clone(&fake.events);
    launch_checked(fake, |exe: &Path| {
        assert!(same_path(exe, &admin_exe(Path::new(r"C:\Program Files"))));
        events.borrow_mut().push("started");
    })
}

#[test]
fn the_start_runs_while_the_hashed_handle_is_held_and_it_is_released_after() {
    let fake = Fake::ready();
    assert_eq!(launch(&fake), Ok(()));
    assert_eq!(*fake.events.borrow(), ["held", "started", "released"]);
}

#[test]
fn a_changed_copy_or_a_failed_check_starts_nothing_and_holds_nothing() {
    let mut changed = Fake::ready();
    changed.hash = HASH.replace('0', "f");
    assert_eq!(launch(&changed), Err(Blocked::AdminCopyChanged));
    // Hashed through the hold, refused, released: never started.
    assert_eq!(*changed.events.borrow(), ["held", "released"]);
    let mut other = Fake::ready();
    other.another = true;
    assert_eq!(launch(&other), Err(Blocked::OtherAccount));
    let mut missing = Fake::ready();
    missing.recorded = None;
    assert_eq!(launch(&missing), Err(Blocked::AdminCopyMissing));
    for fake in [&other, &missing] {
        assert!(
            fake.events.borrow().is_empty(),
            "{:?}",
            fake.events.borrow()
        );
    }
}

#[test]
fn owners_that_cant_be_read_fail_closed() {
    let mut fake = Fake::ready();
    fake.owner_unreadable = true;
    assert_eq!(check_launchable(&fake), Err(Blocked::AdminCopyMissing));
    assert!(fake.asked.borrow().is_empty(), "nothing else is read");
    assert_eq!(launch(&fake), Err(Blocked::AdminCopyMissing));
    assert!(fake.events.borrow().is_empty());
    assert_eq!(
        protection_state(&fake, SetupState::Needed, SetupState::Needed),
        SetupState::Blocked(Blocked::AdminCopyMissing)
    );
    // A pool already set up shows as it is: the launch and the start still refuse.
    assert_eq!(
        protection_state(&fake, SetupState::Ready, SetupState::Ready),
        SetupState::Ready
    );
}

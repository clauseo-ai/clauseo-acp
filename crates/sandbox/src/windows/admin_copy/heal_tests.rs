//! The group's deny healed with no prompt (`heal.rs`): what counts as lacking only the deny,
//! and the lawyer's side asking the refresh task and waiting, bounded, before each check.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use super::fake::{Fake, Task, descriptor, locked_dacl, locked_dacl_denying};
use super::heal::{ASK_EVERY, HEAL_LOOKS, heal_with};
use super::*;

const GROUP: &str = "S-1-5-21-1-2-3-1100";
const OTHER_GROUP: &str = "S-1-5-21-1-2-3-1200";

fn group() -> Sid {
    Sid::parse(GROUP).unwrap()
}

fn sandbox() -> PathBuf {
    admin_dir(Path::new(r"C:\Program Files"))
}

fn exe() -> PathBuf {
    admin_exe(Path::new(r"C:\Program Files"))
}

/// A machine set up before the deny, or refreshed by a copy that had none: every rule holds
/// but the deny, and the record names the group.
fn lacking() -> Fake {
    let mut fake = Fake::ready();
    fake.group = Some(group());
    fake
}

#[test]
fn only_the_exact_old_lock_lacks_the_deny_anything_else_is_drift() {
    let (old_dir, old_exe) = (locked_dacl(true), locked_dacl(false));
    let (new_dir, new_exe) = (
        locked_dacl_denying(true, GROUP),
        locked_dacl_denying(false, GROUP),
    );
    let g = group();
    assert_eq!(owed(&new_dir, &new_exe, &g), Owed::Locked);
    let both = Owed::Lacking {
        folder: true,
        copy: true,
    };
    assert_eq!(owed(&old_dir, &old_exe, &g), both);
    let folder = Owed::Lacking {
        folder: true,
        copy: false,
    };
    assert_eq!(owed(&old_dir, &new_exe, &g), folder);
    let copy = Owed::Lacking {
        folder: false,
        copy: true,
    };
    assert_eq!(owed(&new_dir, &old_exe, &g), copy);
    // The folder's entries on the copy, or the copy's on the folder, are drift.
    assert_eq!(owed(&old_exe, &old_exe, &g), Owed::Drift);
    assert_eq!(owed(&old_dir, &old_dir, &g), Owed::Drift);
    // Another group's deny, an extra entry, an unprotected DACL or another owner: drift.
    let other = locked_dacl_denying(false, OTHER_GROUP);
    assert_eq!(owed(&new_dir, &other, &g), Owed::Drift);
    let extra = descriptor(
        ADMINISTRATORS,
        Some(&[
            (0, 0, FULL_ACCESS, SYSTEM),
            (0, 0, FULL_ACCESS, ADMINISTRATORS),
            (0, 0, READ_EXECUTE, USERS),
            (0, 0, READ_EXECUTE, OTHER_GROUP),
        ]),
        true,
    );
    assert_eq!(owed(&old_dir, &extra, &g), Owed::Drift);
    let entries = [
        (0, 0, FULL_ACCESS, SYSTEM),
        (0, 0, FULL_ACCESS, ADMINISTRATORS),
        (0, 0, READ_EXECUTE, USERS),
    ];
    let unprotected = descriptor(ADMINISTRATORS, Some(&entries), false);
    assert_eq!(owed(&old_dir, &unprotected, &g), Owed::Drift);
    let system_owned = descriptor(SYSTEM, Some(&entries), true);
    assert_eq!(owed(&old_dir, &system_owned, &g), Owed::Drift);
}

#[test]
fn a_copy_lacking_only_the_deny_asks_the_refresh_task_and_passes_once_it_ran() {
    let mut fake = lacking();
    fake.task = Task::HealsAt(1);
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "before");
    let flag = AtomicBool::new(false);
    assert!(heal_with(&fake, &flag));
    assert_eq!((fake.refreshes.get(), fake.pauses.get()), (1, 1));
    assert_eq!(check(&fake), Ok(exe()));
    assert!(!flag.load(Ordering::Relaxed));
    // Healed, nothing is asked again.
    assert!(!heal_with(&fake, &flag));
    assert_eq!(fake.refreshes.get(), 1);
}

#[test]
fn a_run_from_the_old_copy_swaps_but_cant_heal_so_the_task_is_asked_again() {
    let mut fake = lacking();
    fake.task = Task::HealsAt(2);
    assert!(heal_with(&fake, &AtomicBool::new(false)));
    assert_eq!(fake.refreshes.get(), 2);
    assert_eq!(fake.pauses.get(), ASK_EVERY + 1);
    assert_eq!(check(&fake), Ok(exe()));
}

#[test]
fn a_heal_that_never_comes_waits_its_bound_once_then_looks_only_once() {
    let fake = lacking();
    let flag = AtomicBool::new(false);
    assert!(!heal_with(&fake, &flag));
    assert_eq!(fake.pauses.get(), HEAL_LOOKS);
    assert_eq!(fake.refreshes.get(), 1 + HEAL_LOOKS / ASK_EVERY);
    assert!(flag.load(Ordering::Relaxed));
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe));
    assert!(!heal_with(&fake, &flag));
    assert_eq!(fake.pauses.get(), HEAL_LOOKS + 1, "one look");
}

#[test]
fn nothing_is_asked_unless_the_deny_is_the_only_fault() {
    let flag = AtomicBool::new(false);
    let asked = |fake: &Fake, why: &str| {
        assert!(!heal_with(fake, &flag), "{why}");
        assert_eq!((fake.refreshes.get(), fake.pauses.get()), (0, 0), "{why}");
    };
    // No group recorded: no deny is owed.
    asked(&Fake::ready(), "no group");
    // The deny is there.
    let mut fake = lacking();
    fake.set(&sandbox(), Found::Folder, locked_dacl_denying(true, GROUP));
    fake.set(&exe(), Found::File, locked_dacl_denying(false, GROUP));
    asked(&fake, "locked");
    // The copy is drift.
    let mut fake = lacking();
    fake.set(&exe(), Found::File, locked_dacl_denying(false, OTHER_GROUP));
    asked(&fake, "drift");
    // Another rule fails: a stray file in the folder, another hash, a level others can change.
    let mut fake = lacking();
    fake.set(
        &sandbox().join("version.dll"),
        Found::File,
        locked_dacl(false),
    );
    asked(&fake, "stray");
    let mut fake = lacking();
    fake.hash = "f".repeat(64);
    asked(&fake, "hash");
    let mut fake = lacking();
    let open = descriptor(ADMINISTRATORS, Some(&[(0, 0, FULL_ACCESS, USERS)]), false);
    fake.set(Path::new(r"C:\Program Files\Clauseo"), Found::Folder, open);
    asked(&fake, "chain");
    // A group that can't be read.
    let mut fake = lacking();
    fake.group_unreadable = true;
    asked(&fake, "unreadable group");
    assert!(!flag.load(Ordering::Relaxed));
}

#[test]
fn a_task_that_cant_be_started_is_no_wait() {
    let mut fake = lacking();
    fake.task = Task::Fails;
    assert!(!heal_with(&fake, &AtomicBool::new(false)));
    assert_eq!(fake.pauses.get(), 0);
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe));
}

#[test]
fn the_launch_the_set_up_start_and_the_state_each_heal_before_they_check() {
    let mut fake = lacking();
    fake.task = Task::HealsAt(1);
    assert_eq!(check_launchable(&fake), Ok(exe()));
    let mut fake = lacking();
    fake.task = Task::HealsAt(1);
    assert_eq!(launch_checked(&fake, Path::to_path_buf), Ok(exe()));
    let mut fake = lacking();
    fake.task = Task::HealsAt(1);
    let state = protection_state(&fake, SetupState::Ready, SetupState::Ready);
    assert_eq!(state, SetupState::Ready);
    // Not healed: refused as before.
    let mut fake = lacking();
    fake.task = Task::Fails;
    assert_eq!(check_launchable(&fake), Err(Blocked::AdminCopyUnsafe));
}

//! `--remove`'s set-aside bookkeeping against the set-up's fake: a forgotten matter's
//! `record.json` and `set-aside.lock` go as the owner, but never a record that still lists an
//! open session, never through a link, and never for a matter whose forget failed. On every OS
//! the fake calls the real deletion (`set_aside/bookkeeping.rs`): NTFS by handle on Windows, the
//! stand-in elsewhere.

use std::path::Path;

use clauseo_protocol::MatterId;

use super::remove::{assert_all_gone, pool};
use super::remove_fake::Remover;
use super::remove_ledger::{CLOSED, bookkeeping, matter_one, owners_matter};
use super::*;
use crate::windows::write;
use crate::windows::{SET_ASIDE_BOOKKEEPING, SET_ASIDE_DIR};

/// A record whose session never had its after-session pass: a crash's changes still unchecked.
const OPEN: &[u8] = br#"{"version": 1, "sessions": ["s1"], "names": []}"#;

/// Makes `link`, in `machine`'s scratch folder, a link to the folder `target`: a junction on
/// Windows, a symbolic link elsewhere.
fn folder_link(machine: &Remover, target: &Path, link: &Path) {
    #[cfg(windows)]
    {
        let _ = machine;
        crate::windows::test_junction::junction(link, target).unwrap();
    }
    #[cfg(not(windows))]
    symlink(machine, target, link);
}

/// Makes `link`, in `machine`'s scratch folder, a symbolic link to `target`.
#[cfg(unix)]
fn symlink(machine: &Remover, target: &Path, link: &Path) {
    let rel = link.strip_prefix(machine.m.dir.path()).unwrap();
    machine.m.dir.symlink(rel.to_str().unwrap(), target);
}

#[test]
fn a_record_still_listing_an_open_session_is_kept_with_its_lock() {
    // The next launch checks a crashed session's changes only against it (`run.rs`).
    let (mut machine, mut acl) = pool("remove-set-aside-open", Some(owner()));
    let data_dir = machine.m.data_dir();
    owners_matter(&machine);
    let one = bookkeeping(&data_dir, matter_one());
    write::write_atomic(&one.join("record.json"), OPEN).unwrap();
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    assert_eq!(machine.forgot, [matter_one()]);
    assert_eq!(std::fs::read(one.join("record.json")).unwrap(), OPEN);
    assert!(one.join("set-aside.lock").exists());
    assert_all_gone(&machine, &acl);
}

#[test]
fn a_matter_whose_forget_failed_or_left_an_entry_keeps_its_bookkeeping() {
    // A removal of one of its entries fails: the entry stays in the ledger, and so does the
    // matter's record, which the next try (after a reinstall) still checks against.
    let (mut machine, mut acl) = pool("remove-set-aside-forget-fails", Some(owner()));
    owners_matter(&machine);
    let one = bookkeeping(&machine.m.data_dir(), matter_one());
    let mut probe = pool("remove-set-aside-forget-probe", Some(owner()));
    owners_matter(&probe.0);
    remove(&mut probe.0, &mut probe.1, false).unwrap();
    let at = probe
        .0
        .lines()
        .iter()
        .position(|l| l == "remove matters/m1 1001");
    machine.fail_at.set(at);
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(failure.step, RemoveStep::Ledger);
    assert_eq!(machine.forgot, Vec::<MatterId>::new());
    assert_eq!(std::fs::read(one.join("record.json")).unwrap(), CLOSED);
    assert!(one.join("set-aside.lock").exists());
}

#[test]
fn a_matter_folder_that_is_a_link_is_not_followed() {
    let (mut machine, mut acl) = pool("remove-set-aside-matter-link", Some(owner()));
    let data_dir = machine.m.data_dir();
    owners_matter(&machine);
    // The real folder elsewhere, holding what looks like bookkeeping; the matter's name links to
    // it.
    let elsewhere = machine.m.dir.path().join("elsewhere");
    let decoy = bookkeeping(&elsewhere, matter_one());
    let set_aside = data_dir
        .join(crate::windows::matters::SANDBOX_DIR)
        .join(SET_ASIDE_DIR);
    write::make_sandbox_dir(&set_aside).unwrap();
    let one = set_aside.join(matter_one().to_string());
    folder_link(&machine, &decoy, &one);
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    for name in SET_ASIDE_BOOKKEEPING {
        assert!(decoy.join(name).exists(), "{name}");
    }
    assert!(std::fs::symlink_metadata(&one).is_ok());
    assert_all_gone(&machine, &acl);
}

#[cfg(unix)]
#[test]
fn a_record_or_a_set_aside_that_is_a_symbolic_link_is_not_followed() {
    let (mut machine, mut acl) = pool("remove-set-aside-record-link", Some(owner()));
    let data_dir = machine.m.data_dir();
    owners_matter(&machine);
    let one = bookkeeping(&data_dir, matter_one());
    let lawyers = machine.m.dir.path().join("lawyers.json");
    write::write_atomic(&lawyers, CLOSED).unwrap();
    write::remove_owned_file(&one.join("record.json")).unwrap();
    symlink(&machine, &lawyers, &one.join("record.json"));
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    assert_eq!(std::fs::read(&lawyers).unwrap(), CLOSED);
    assert!(one.join("set-aside.lock").exists());

    // `set-aside` itself a link: refused, as NTFS's `folder_chain` refuses it.
    let (mut machine, mut acl) = pool("remove-set-aside-dir-link", Some(owner()));
    let data_dir = machine.m.data_dir();
    owners_matter(&machine);
    let elsewhere = machine.m.dir.path().join("elsewhere");
    let decoy = bookkeeping(&elsewhere, matter_one());
    let sandbox = data_dir.join(crate::windows::matters::SANDBOX_DIR);
    write::make_sandbox_dir(&sandbox).unwrap();
    symlink(
        &machine,
        decoy.parent().unwrap(),
        &sandbox.join(SET_ASIDE_DIR),
    );
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    for name in SET_ASIDE_BOOKKEEPING {
        assert!(decoy.join(name).exists(), "{name}");
    }
}

#[test]
fn bookkeeping_names_match_ignoring_case_as_ntfs_does() {
    let (mut machine, mut acl) = pool("remove-set-aside-case", Some(owner()));
    let data_dir = machine.m.data_dir();
    owners_matter(&machine);
    let one = bookkeeping(&data_dir, matter_one());
    write::remove_owned_file(&one.join("set-aside.lock")).unwrap();
    write::write_atomic(&one.join("Set-Aside.LOCK"), b"").unwrap();
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    assert!(
        std::fs::symlink_metadata(&one).is_err(),
        "{}",
        one.display()
    );
}

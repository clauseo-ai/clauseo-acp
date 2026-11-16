//! `--remove`'s set-aside bookkeeping against the set-up's fake: a forgotten matter's
//! `record.json` and `set-aside.lock` go as the owner, but never a record that still lists an
//! open session, never through a link, and never for a matter whose forget failed. On every OS
//! the fake calls the real deletion (`set_aside/bookkeeping.rs`): NTFS by handle on Windows, the
//! stand-in elsewhere.

use std::path::Path;

use clauseo_protocol::MatterId;

use super::remove::{assert_all_gone, pool};
use super::remove_fake::Remover;
use super::remove_ledger::{CLOSED, LEFT_BY_LAUNCH, bookkeeping, matter_one, owners_matter};
use super::*;
use crate::windows::SET_ASIDE_DIR;
use crate::windows::write;

/// A record whose session never had its after-session pass: a crash's changes still unchecked.
const OPEN: &[u8] = b"{\"version\": 1, \"matter\": \"01890a5d-ac96-774b-bcce-b302099a8057\", \
    \"sessions\": [\"s1\"], \"names\": []}";

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
    for name in LEFT_BY_LAUNCH {
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
    for name in LEFT_BY_LAUNCH {
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

/// A manifest with nothing outstanding: every item restored (`files.rs`'s typed form).
use crate::windows::set_aside::tests::{MANIFEST_NONE, MANIFEST_ONE};

/// A removal as the owner of a forgotten matter whose set-aside folder holds the launch's
/// bookkeeping, `manifest` as its manifest, and what `also` adds; `check` then looks at that
/// folder while the scratch folder is still there.
fn removed_with_manifest(
    test: &str,
    manifest: &[u8],
    also: impl FnOnce(&Remover, &Path),
    check: impl FnOnce(&Path),
) {
    let (mut machine, mut acl) = pool(test, Some(owner()));
    let data_dir = machine.m.data_dir();
    owners_matter(&machine);
    let one = bookkeeping(&data_dir, matter_one());
    write::write_atomic(&one.join("manifest.json"), manifest).unwrap();
    also(&machine, &one);
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    assert_eq!(machine.forgot, [matter_one()]);
    assert_all_gone(&machine, &acl);
    check(&one);
}

/// `folder` is there with the launch's bookkeeping and its manifest, untouched.
fn assert_untouched(folder: &Path) {
    for name in LEFT_BY_LAUNCH.iter().chain(&["manifest.json"]) {
        assert!(folder.join(name).exists(), "{name}");
    }
}

/// `rel`, `path` below `machine`'s scratch folder, for its helpers.
fn rel(machine: &Remover, path: &Path) -> String {
    let below = path.strip_prefix(machine.m.dir.path()).unwrap();
    below.to_str().unwrap().to_owned()
}

#[test]
fn a_fully_restored_matter_goes_whole() {
    removed_with_manifest(
        "remove-set-aside-restored",
        MANIFEST_NONE,
        |_, _| {},
        |one| {
            assert!(std::fs::symlink_metadata(one).is_err(), "{}", one.display());
        },
    );
}

#[test]
fn a_manifest_still_listing_an_item_keeps_everything() {
    removed_with_manifest(
        "remove-set-aside-outstanding",
        MANIFEST_ONE,
        |_, _| {},
        assert_untouched,
    );
}

#[test]
fn a_manifest_listing_nothing_beside_a_pass_folder_with_a_file_keeps_everything() {
    let pass = |_: &Remover, one: &Path| {
        let git = one.join("20261107T101500-000Z").join(".git");
        write::make_sandbox_dir(&git).unwrap();
        write::write_atomic(&git.join("HEAD"), b"ref: refs/heads/main").unwrap();
    };
    removed_with_manifest("remove-set-aside-pass", MANIFEST_NONE, pass, |one| {
        assert_untouched(one);
        assert!(one.join("20261107T101500-000Z/.git/HEAD").exists());
    });
}

#[test]
fn an_unparsable_or_other_version_manifest_keeps_everything() {
    let other = String::from_utf8(MANIFEST_NONE.to_vec()).unwrap().replacen(
        "\"version\": 1",
        "\"version\": 2",
        1,
    );
    assert_ne!(other.as_bytes(), MANIFEST_NONE);
    for (n, manifest) in [b"{}".as_slice(), b"data", other.as_bytes()]
        .into_iter()
        .enumerate()
    {
        let test = format!("remove-set-aside-manifest-{n}");
        removed_with_manifest(&test, manifest, |_, _| {}, assert_untouched);
    }
}

#[test]
fn a_hard_linked_manifest_or_lock_keeps_everything_the_record_included() {
    for name in ["manifest.json", "set-aside.lock"] {
        let link = |machine: &Remover, one: &Path| {
            let original = machine.m.dir.path().join("lawyers-file");
            write::write_atomic(&original, MANIFEST_NONE).unwrap();
            write::remove_owned_file(&one.join(name)).unwrap();
            machine
                .m
                .dir
                .hard_link(&rel(machine, &one.join(name)), &original);
        };
        let test = format!("remove-set-aside-hard-link-{name}");
        removed_with_manifest(&test, MANIFEST_NONE, link, |one| {
            assert_eq!(std::fs::read(one.join("record.json")).unwrap(), CLOSED);
            assert_untouched(one);
        });
    }
}

//! The admin copy's checks on NTFS (Windows only): descriptors as Windows writes and reads
//! them. The lock-down and the full chain need an elevated token (to make Administrators the
//! owner), so that test is an `elevated_` one, run by CI's elevated step.

use std::io;
use std::path::{Path, PathBuf};

use super::os::{self, WindowsAdminCopy, lock_down, lock_down_tree};
use super::*;
use crate::test_support::TempDir;
use crate::windows::test_acl::elevated_tests_enabled;
use crate::windows::test_dacl::protect_with;

/// The locked folder's DACL with a Users entry for write and append planted in it.
const PLANTED: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200af;;;BU)";

/// A Program Files stand-in at `root`, read from NTFS, with `recorded` as the record's hash.
/// The folders above it (the runner's temporary folders and drive) aren't the test's: they read
/// as a locked folder would, the real ones being what the fakes test.
struct Rooted {
    root: PathBuf,
    recorded: Option<String>,
    /// The record's `GroupSid`.
    group: Option<Sid>,
}

impl Rooted {
    /// Whether `path` is a folder above the stand-in.
    fn above(&self, path: &Path) -> bool {
        inside(&self.root, path)
    }
}

impl AdminCopyFs for Rooted {
    fn program_files(&self) -> io::Result<PathBuf> {
        Ok(self.root.clone())
    }

    fn found(&self, path: &Path) -> io::Result<Found> {
        if self.above(path) {
            return Ok(Found::Folder);
        }
        os::found(path)
    }

    fn security(&self, path: &Path) -> io::Result<Vec<u8>> {
        if self.above(path) {
            return Ok(super::fake::locked_dacl(true));
        }
        os::security(path)
    }

    fn sha256(&self, path: &Path) -> io::Result<String> {
        os::sha256(path)
    }

    fn sha256_held(&self, path: &Path) -> io::Result<(String, Held)> {
        os::sha256_held(path)
    }

    fn recorded_sha256(&self) -> io::Result<Option<String>> {
        Ok(self.recorded.clone())
    }

    fn recorded_group(&self) -> io::Result<Option<Sid>> {
        Ok(self.group.clone())
    }

    fn entries(&self, dir: &Path) -> io::Result<Vec<String>> {
        WindowsAdminCopy.entries(dir)
    }

    fn another_owner(&self) -> io::Result<bool> {
        Ok(false)
    }

    fn security_blocked(&self) -> io::Result<bool> {
        Ok(false)
    }

    fn set_security_blocked(&self, _blocked: bool) -> io::Result<()> {
        Ok(())
    }
}

/// Whether the DACL in `bytes` has an allow entry for Users with a change right.
fn users_can_change(bytes: &[u8]) -> bool {
    let dacl = Descriptor::parse(bytes).unwrap().dacl.unwrap();
    Acl::parse(dacl).unwrap().aces().iter().any(|ace| {
        acl_bytes::kind(ace) == acl_bytes::ALLOWED
            && acl_bytes::sid(ace)
                .and_then(acl_bytes::sid_text)
                .is_some_and(|sid| sid.as_str() == USERS)
            && acl_bytes::mask(ace).is_some_and(|mask| mask & CHANGE_RIGHTS != 0)
    })
}

#[test]
fn a_planted_users_write_reads_back_from_ntfs_and_the_level_is_unsafe() {
    let dir = TempDir::new("admin-copy-planted");
    let folder = dir.dir("Sandbox");
    protect_with(&folder, PLANTED).unwrap();
    let bytes = os::security(&folder).unwrap();
    assert!(
        users_can_change(&bytes),
        "the planted entry reads back with its write bits"
    );
    assert!(!level_safe(&bytes));
    assert_eq!(os::found(&folder).unwrap(), Found::Folder);
}

#[test]
fn the_hash_is_the_files_sha256_and_a_missing_file_is_missing() {
    let dir = TempDir::new("admin-copy-hash");
    let file = dir.file("x.exe", "abc");
    // SHA-256("abc").
    assert_eq!(
        os::sha256(&file).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(os::found(&dir.path().join("none")).unwrap(), Found::Missing);
}

/// A held hash keeps the file from being written, renamed or deleted until the hold drops, and
/// still lets it be read (as starting it does).
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test's own file: written, renamed and deleted to see the hold refuse each"
)]
fn a_held_hash_keeps_the_file_from_changing_until_it_drops() {
    let dir = TempDir::new("admin-copy-held");
    let file = dir.file("x.exe", "abc");
    let (hash, held) = os::sha256_held(&file).unwrap();
    assert_eq!(hash, os::sha256(&file).unwrap(), "read while held");
    let write = std::fs::OpenOptions::new().write(true).open(&file);
    assert!(write.is_err(), "no write while held");
    assert!(
        std::fs::rename(&file, dir.path().join("y.exe")).is_err(),
        "no rename"
    );
    assert!(std::fs::remove_file(&file).is_err(), "no delete");
    drop(held);
    std::fs::remove_file(&file).unwrap();
}

#[test]
#[ignore = "elevated: makes Administrators the owner; CI's elevated step runs it"]
fn elevated_admin_copy_lock_down_reads_back_and_a_planted_users_write_refuses() {
    if !elevated_tests_enabled() {
        return;
    }
    let dir = TempDir::new("admin-copy-elevated");
    let root = dir.path().join("pf");
    let exe = dir.file(
        r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
        "MZ stand-in",
    );
    assert!(same_path(&exe, &admin_exe(&root)));
    let mut levels = vec![root.clone()];
    for part in ADMIN_DIR {
        let next = levels.last().unwrap().join(part);
        levels.push(next);
    }
    for level in &levels {
        lock_down(level, true, None).unwrap();
        let bytes = os::security(level).unwrap();
        assert!(
            locked(&bytes, true, None) && level_safe(&bytes),
            "{}",
            level.display()
        );
    }
    lock_down(&exe, false, None).unwrap();
    let back = os::security(&exe).unwrap();
    assert!(
        locked(&back, false, None) && level_safe(&back),
        "the copy reads back locked"
    );
    let recorded = Some(os::sha256(&exe).unwrap());
    let fs = Rooted {
        root,
        recorded,
        group: None,
    };
    assert!(check(&fs).is_ok_and(|path| same_path(&path, &exe)));
    // Users given write on the Sandbox folder: unsafe, and the read-back no longer `locked`.
    let sandbox = levels.last().unwrap();
    protect_with(sandbox, PLANTED).unwrap();
    let planted = os::security(sandbox).unwrap();
    assert!(!locked(&planted, true, None) && !level_safe(&planted));
    assert_eq!(check(&fs), Err(Blocked::AdminCopyUnsafe));
    // A stray beside the copy refuses; the lock-down deletes it and locks what stays.
    let stray = dir.file(r"pf\Clauseo\Sandbox\version.dll", "planted");
    assert_eq!(check(&fs), Err(Blocked::AdminCopyUnsafe));
    lock_down_tree(sandbox, &exe, None).unwrap();
    assert_eq!(os::found(&stray).unwrap(), Found::Missing);
    assert!(locked(&os::security(sandbox).unwrap(), true, None));
    assert!(check(&fs).is_ok());
    // A changed copy is changed.
    dir.file(
        r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
        "MZ another",
    );
    lock_down(&exe, false, None).unwrap();
    assert_eq!(check(&fs), Err(Blocked::AdminCopyChanged));
}

/// The lock with the pool's group denied, as Windows writes and reads it: the set-up's tree
/// lock reads back exactly ([`locked`] with the group), the check passes with that group
/// recorded, and a lock with no deny, read against the recorded group, is unsafe until the
/// refresh's heal (`os::heal_deny`) gives it the deny; drift it leaves.
#[test]
#[ignore = "elevated: makes Administrators the owner; CI's elevated step runs it"]
fn elevated_the_lock_denies_the_recorded_group_and_the_check_requires_it() {
    if !elevated_tests_enabled() {
        return;
    }
    let group = Sid::parse("S-1-5-21-1-2-3-1100").unwrap();
    let dir = TempDir::new("admin-copy-deny");
    let root = dir.path().join("pf");
    let exe = dir.file(
        r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
        "MZ stand-in",
    );
    let sandbox = admin_dir(&root);
    lock_down(&root, true, None).unwrap();
    lock_down(&root.join(ADMIN_DIR[0]), true, None).unwrap();
    lock_down_tree(&sandbox, &exe, Some(&group)).unwrap();
    for (path, folder) in [(&sandbox, true), (&exe, false)] {
        let bytes = os::security(path).unwrap();
        assert!(locked(&bytes, folder, Some(&group)), "{}", path.display());
        assert!(!locked(&bytes, folder, None), "{}", path.display());
        assert!(group_denied(&bytes, &group) && level_safe(&bytes));
    }
    let recorded = Some(os::sha256(&exe).unwrap());
    let fs = Rooted {
        root,
        recorded,
        group: Some(group.clone()),
    };
    assert!(check(&fs).is_ok_and(|path| same_path(&path, &exe)));
    lock_down_tree(&sandbox, &exe, None).unwrap();
    assert_eq!(check(&fs), Err(Blocked::AdminCopyUnsafe));
    // The refresh's heal: the old lock gets the deny and passes; a second heal changes nothing.
    assert!(os::heal_deny(&sandbox, &exe, &group).unwrap());
    assert!(check(&fs).is_ok());
    assert!(!os::heal_deny(&sandbox, &exe, &group).unwrap());
    // Drift is left as it is.
    protect_with(&exe, OWN_FILE).unwrap();
    assert!(!os::heal_deny(&sandbox, &exe, &group).unwrap());
    assert!(!locked(&os::security(&exe).unwrap(), false, Some(&group)));
}

/// A protected DACL no lock-down writes, so a descriptor left alone reads back as it was: the
/// locked folder's inheritable entries don't reach it.
const OWN_FILE: &str = "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)";

/// [`OWN_FILE`] for a folder and what is made in it.
const OWN_FOLDER: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;BU)";

/// The admin copy given a second name outside the admin folder (a hard link): the lock-down
/// refuses it, as `os::lock_handle` says, and never locks it; the descriptor both names share
/// reads back as it was, and both names stay.
#[test]
#[ignore = "elevated: makes Administrators the owner; CI's elevated step runs it"]
fn elevated_the_lock_down_refuses_a_file_with_two_names_and_leaves_it() {
    use super::lock_tree_os::lock_tree;
    if !elevated_tests_enabled() {
        return;
    }
    let dir = TempDir::new("admin-copy-hard-link");
    let exe = dir.file(r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe", "MZ");
    let sandbox = exe.parent().unwrap().to_path_buf();
    protect_with(&exe, OWN_FILE).unwrap();
    let other = dir.hard_link("elsewhere.exe", &exe);
    let before = os::security(&other).unwrap();
    for refused in [
        lock_tree(&sandbox, None, None),
        lock_down_tree(&sandbox, &exe, None),
    ] {
        let error = refused.unwrap_err();
        assert!(error.to_string().contains("more than one name"), "{error}");
        assert_eq!(
            os::security(&other).unwrap(),
            before,
            "the other name's, untouched"
        );
        assert_eq!(os::security(&exe).unwrap(), before, "the copy's, untouched");
        assert!(!locked(&os::security(&exe).unwrap(), false, None));
        assert_eq!(os::found(&exe).unwrap(), Found::File);
        assert_eq!(os::found(&other).unwrap(), Found::File);
    }
}

/// A junction in the admin folder, to a folder outside it: the lock-down's walk deletes the
/// junction as itself and never follows it, so the folder it names and the file in it keep
/// their descriptors; the admin folder itself being a junction is refused, nothing changed.
#[test]
#[ignore = "elevated: makes Administrators the owner; CI's elevated step runs it"]
fn elevated_the_lock_down_never_follows_a_junction_in_the_admin_folder() {
    use super::lock_tree_os::lock_tree;
    use crate::windows::test_junction::junction;
    if !elevated_tests_enabled() {
        return;
    }
    let dir = TempDir::new("admin-copy-junction");
    let exe = dir.file(r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe", "MZ");
    let sandbox = exe.parent().unwrap().to_path_buf();
    let target = dir.dir("target");
    protect_with(&target, OWN_FOLDER).unwrap();
    let inner = dir.file(r"target\inner.txt", "the lawyer's");
    let (folder_before, file_before) = (
        os::security(&target).unwrap(),
        os::security(&inner).unwrap(),
    );
    let untouched = || {
        assert_eq!(os::security(&target).unwrap(), folder_before);
        assert_eq!(os::security(&inner).unwrap(), file_before);
        assert_eq!(os::found(&inner).unwrap(), Found::File);
    };
    // Below the admin folder: deleted as itself, the rest locked.
    let link = sandbox.join("linked");
    junction(&link, &target).unwrap();
    lock_tree(&sandbox, None, None).unwrap();
    assert_eq!(os::found(&link).unwrap(), Found::Missing);
    assert!(locked(&os::security(&sandbox).unwrap(), true, None));
    assert!(locked(&os::security(&exe).unwrap(), false, None));
    untouched();
    // The admin folder itself a junction: refused, and the junction stays.
    let root = dir.path().join("root-link");
    junction(&root, &target).unwrap();
    assert!(lock_tree(&root, None, None).is_err());
    assert_eq!(os::found(&root).unwrap(), Found::Link);
    untouched();
}

/// `path` made SYSTEM's alone, owner and protected DACL, as a refresh made `staging\` under the
/// old `STAGING_SDDL`; `SeRestorePrivilege` back as it was after.
fn system_only(path: &Path, folder: bool) {
    use crate::windows::acl::privilege::Privileges;
    let system = crate::windows::sids::Sid::parse("S-1-5-18").unwrap();
    let _restore = Privileges::enable(&["SeRestorePrivilege"]).unwrap();
    crate::windows::test_dacl::give_owner(path, &system).unwrap();
    let dacl = if folder {
        "D:P(A;OICI;FA;;;SY)"
    } else {
        "D:P(A;;FA;;;SY)"
    };
    protect_with(path, dacl).unwrap();
}

/// Whether the descriptor in `bytes` is SYSTEM's alone: owned by SYSTEM, a protected DACL, and
/// allow entries for SYSTEM and nobody else.
fn system_alone(bytes: &[u8]) -> bool {
    if acl_bytes::owner_sid(bytes)
        .as_ref()
        .map(crate::windows::sids::Sid::as_str)
        != Some(SYSTEM)
    {
        return false;
    }
    let Ok(descriptor) = Descriptor::parse(bytes) else {
        return false;
    };
    let Some(dacl) = descriptor.dacl.filter(|_| descriptor.dacl_protected()) else {
        return false;
    };
    let Ok(dacl) = Acl::parse(dacl) else {
        return false;
    };
    let aces = dacl.aces();
    !aces.is_empty()
        && aces.iter().all(|ace| {
            acl_bytes::kind(ace) == acl_bytes::ALLOWED
                && acl_bytes::sid(ace)
                    .and_then(acl_bytes::sid_text)
                    .is_some_and(|sid| sid.as_str() == SYSTEM)
        })
}

/// A Sandbox stand-in with the copy and a `staging\` holding a set-aside `.old`, both SYSTEM's
/// alone: what a successful refresh leaves until the restart.
fn shut_staging(dir: &TempDir) -> (PathBuf, PathBuf, PathBuf) {
    let exe = dir.file(r"pf\Clauseo\Sandbox\clauseo-sandbox-setup.exe", "MZ");
    let old = dir.file(
        r"pf\Clauseo\Sandbox\staging\clauseo-sandbox-setup.exe.old",
        "MZ old",
    );
    let staging = old.parent().unwrap().to_path_buf();
    system_only(&old, false);
    system_only(&staging, true);
    // Not a `read_dir` check: it opens with backup intent, so a token with an enabled
    // SeBackupPrivilege lists the folder anyway. The descriptor is what shuts it.
    let bytes = os::security(&staging).unwrap();
    assert!(system_alone(&bytes), "shut to SYSTEM alone");
    (exe.parent().unwrap().to_path_buf(), exe, staging)
}

#[test]
#[ignore = "elevated: takes a SYSTEM-only folder back; CI's elevated step runs it"]
fn elevated_a_system_only_staging_is_taken_back_by_the_lock_down_and_the_teardown() {
    use super::take_back::{LockDownFs as _, remove_taking_back};
    use super::take_back_os::NtfsLockDown;
    if !elevated_tests_enabled() {
        return;
    }
    // --adopt (a repair after a refresh): staging goes, the rest is locked.
    let dir = TempDir::new("admin-copy-take-back-adopt");
    let (sandbox, exe, staging) = shut_staging(&dir);
    lock_down_tree(&sandbox, &exe, None).unwrap();
    assert_eq!(os::found(&staging).unwrap(), Found::Missing);
    assert!(locked(&os::security(&sandbox).unwrap(), true, None));
    assert!(locked(&os::security(&exe).unwrap(), false, None));
    // --remove --uninstall's teardown: deleted.
    let dir = TempDir::new("admin-copy-take-back-remove");
    let (_sandbox, _exe, staging) = shut_staging(&dir);
    remove_taking_back(&mut NtfsLockDown::new(None), &staging).unwrap();
    assert_eq!(os::found(&staging).unwrap(), Found::Missing);
    // The take-back alone: re-secured as the locked folder, and readable again.
    let dir = TempDir::new("admin-copy-take-back-secure");
    let (_sandbox, _exe, staging) = shut_staging(&dir);
    NtfsLockDown::new(None).take_back(&staging).unwrap();
    assert!(locked(&os::security(&staging).unwrap(), true, None));
    assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 1);
}

/// The take-back of a file, opened once as itself: a SYSTEM-only file comes back locked and
/// readable; one with a second name is refused and left; a link is left, never taken back
/// through, and what it names keeps its descriptor.
#[test]
#[ignore = "elevated: takes a SYSTEM-only file back; CI's elevated step runs it"]
fn elevated_a_file_is_taken_back_through_its_own_handle() {
    use super::take_back::LockDownFs as _;
    use super::take_back_os::NtfsLockDown;
    use crate::windows::test_junction::junction;
    if !elevated_tests_enabled() {
        return;
    }
    let dir = TempDir::new("admin-copy-take-back-file");
    let (_sandbox, _exe, staging) = shut_staging(&dir);
    let old = staging.join("clauseo-sandbox-setup.exe.old");
    NtfsLockDown::new(None).take_back(&old).unwrap();
    assert!(locked(&os::security(&old).unwrap(), false, None));
    assert_eq!(
        os::sha256(&old).unwrap(),
        os::sha256(&dir.file("same", "MZ old")).unwrap()
    );
    // Two names: refused, the descriptor as it was.
    let linked = dir.file("linked.exe", "MZ");
    dir.hard_link("second.exe", &linked);
    system_only(&linked, false);
    let before = os::security(&linked).unwrap();
    assert!(NtfsLockDown::new(None).take_back(&linked).is_err());
    assert_eq!(os::security(&linked).unwrap(), before);
    // A junction: left, and what it names untouched.
    let target = dir.dir("target");
    protect_with(&target, OWN_FOLDER).unwrap();
    let target_before = os::security(&target).unwrap();
    let link = dir.path().join("link");
    junction(&link, &target).unwrap();
    NtfsLockDown::new(None).take_back(&link).unwrap();
    assert_eq!(os::found(&link).unwrap(), Found::Link);
    assert_eq!(os::security(&target).unwrap(), target_before);
    // Nothing there: nothing to do.
    NtfsLockDown::new(None)
        .take_back(&dir.path().join("none"))
        .unwrap();
}

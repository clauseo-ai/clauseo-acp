//! The shared places' plan, apply and removal against a fake writer, on every OS. The real-NTFS
//! round trip is at the end, for Windows only.

use std::io;
use std::path::{Path, PathBuf};

use super::*;
use crate::test_support::TempDir;
use crate::windows::acl_plan::Sid;
use crate::windows::ledger::RecordedFile;
use crate::windows::test_world::{GROUP_SID, OTHER_POOL_SIDS, USER_SID};
use crate::windows::writer::{FileId, Label, Strict, Walked};

/// Records each call. The `fail_at`-th add (1-based) fails; the `already_at`-th add answers
/// [`Added::AlreadyThere`]; the `fail_walk_at`-th walk fails. Each walk reports `walk_skips`
/// skipped files, and is recorded as its root, its reach and its strict SIDs.
#[derive(Default)]
struct Fake {
    adds: Vec<Grant>,
    removes: Vec<Grant>,
    walks: Vec<(PathBuf, Reach, Option<Strict>)>,
    walk_skips: u64,
    fail_at: Option<usize>,
    already_at: Option<usize>,
    fail_walk_at: Option<usize>,
}

impl AclWriter for Fake {
    fn add(&mut self, grant: &Grant) -> Result<Added, ApplyFailure> {
        self.adds.push(grant.clone());
        let call = self.adds.len();
        if self.fail_at == Some(call) {
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        if self.already_at == Some(call) {
            return Ok(Added::AlreadyThere);
        }
        Ok(Added::Written)
    }

    fn remove(&mut self, grant: &Grant) -> Result<(), ApplyFailure> {
        self.removes.push(grant.clone());
        Ok(())
    }

    fn present(&mut self, _grant: &Grant) -> io::Result<bool> {
        Ok(false)
    }

    fn propagate(
        &mut self,
        root: &Path,
        reach: Reach,
        strict: Option<&Strict>,
    ) -> Result<Walked, ApplyFailure> {
        self.walks.push((root.to_owned(), reach, strict.cloned()));
        if self.fail_walk_at == Some(self.walks.len()) {
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        Ok(Walked {
            skipped: self.walk_skips,
            ..Walked::default()
        })
    }

    fn remove_recorded(
        &mut self,
        _file: &RecordedFile,
        grant: &Grant,
        _prior: Option<Label>,
    ) -> Result<(), ApplyFailure> {
        self.removes.push(grant.clone());
        Ok(())
    }

    fn present_recorded(&mut self, _file: &RecordedFile, _grant: &Grant) -> io::Result<bool> {
        Ok(false)
    }

    fn same_file(&mut self, _a: &Path, _b: &Path) -> io::Result<bool> {
        Ok(false)
    }

    fn file_id(&mut self, _path: &Path) -> io::Result<Option<FileId>> {
        Ok(None)
    }

    fn add_expecting(&mut self, grant: &Grant, _expected: &Path) -> Result<Added, ApplyFailure> {
        self.add(grant)
    }

    fn refused_at(&mut self) -> Option<PathBuf> {
        None
    }

    fn reclaim(&mut self, _root: &Path, _pool: &[Sid]) -> Result<Walked, ApplyFailure> {
        Ok(Walked::default())
    }

    fn label(&mut self, _path: &Path) -> io::Result<Option<Label>> {
        Ok(None)
    }

    fn restore_label(&mut self, _grant: &Grant, _label: Label) -> Result<(), ApplyFailure> {
        Ok(())
    }
}

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

fn group() -> Sid {
    sid(GROUP_SID)
}

fn owner() -> Sid {
    sid(OWNER_RIGHTS_SID)
}

/// The pool users: the one above and the other pool user.
fn pool() -> Vec<Sid> {
    vec![sid(USER_SID), sid(OTHER_POOL_SIDS[0])]
}

fn plan(targets: &[PathBuf]) -> Vec<Grant> {
    plan_shared_write_denies(&group(), targets).unwrap()
}

/// A temp folder and the folders `names` under it (nested names made with their parents).
fn folders(test: &str, names: &[&str]) -> (TempDir, Vec<PathBuf>) {
    let dir = TempDir::new(test);
    let paths = names.iter().map(|name| dir.dir(name)).collect();
    (dir, paths)
}

/// The grants of one target, as `plan` makes them, for a whole-target comparison.
fn on(path: &Path, grants: &[Grant]) -> Vec<Grant> {
    grants
        .iter()
        .filter(|grant| grant.path() == path)
        .cloned()
        .collect()
}

#[test]
fn the_targets_are_device_sync_then_each_pool_profiles_local_low_in_order() {
    let profiles = [PathBuf::from("users/a"), PathBuf::from("users/b")];
    let targets = shared_write_targets(Path::new("pd"), &profiles);
    assert_eq!(
        targets,
        [
            Path::new("pd").join("Microsoft").join("DeviceSync"),
            Path::new("users/a").join("AppData").join("LocalLow"),
            Path::new("users/b").join("AppData").join("LocalLow"),
        ]
    );
}

#[test]
fn no_pool_profiles_leaves_only_device_sync() {
    let targets = shared_write_targets(Path::new("pd"), &[]);
    assert_eq!(targets.len(), 1);
    assert!(targets[0].ends_with(Path::new("Microsoft").join("DeviceSync")));
}

#[test]
fn device_sync_gets_only_the_group_deny_write_as_a_tree() {
    let target = PathBuf::from("pd/Microsoft/DeviceSync");
    let grants = plan(std::slice::from_ref(&target));
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].path(), target.as_path());
    assert_eq!(grants[0].sid(), &group());
    assert_eq!(grants[0].rights(), Rights::DenyWrite);
    assert_eq!(grants[0].rights().mask(), 0x000d_0156);
    assert_eq!(grants[0].inherit(), Inherit::Tree);
    assert_eq!(grants[0].kind(), PathKind::SharedWrite);
}

#[test]
fn local_low_gets_an_owner_rights_deny_right_after_the_groups() {
    let target = PathBuf::from("u/AppData/LocalLow");
    let grants = plan(std::slice::from_ref(&target));
    assert_eq!(grants.len(), 2);
    assert_eq!(grants[0].sid(), &group());
    assert_eq!(grants[1].sid(), &owner());
    for grant in &grants {
        assert_eq!(grant.path(), target.as_path());
        assert_eq!(grant.rights(), Rights::DenyWrite);
        assert_eq!(grant.rights().mask(), 0x000d_0156);
        assert_eq!(grant.kind(), PathKind::SharedWrite);
    }
    // The group's deny covers LocalLow and below; the owner's is inherit-only, for the children.
    assert_eq!(grants[0].inherit(), Inherit::Tree);
    assert_eq!(grants[1].inherit(), Inherit::Below);
}

#[test]
fn the_local_low_test_ignores_case() {
    let grants = plan(&[PathBuf::from("u/appdata/locallow")]);
    assert!(grants.iter().any(|grant| grant.sid() == &owner()));
}

#[test]
fn a_missing_device_sync_is_skipped_and_a_missing_local_low_fails() {
    let (dir, paths) = folders("shared-missing", &["present"]);
    let device = dir.path().join("pd/Microsoft/DeviceSync");
    let low = dir.path().join("u/AppData/LocalLow");
    let targets = [device.clone(), low.clone(), paths[0].clone()];
    let grants = plan(&targets);
    let mut fake = Fake::default();
    let done = apply_shared_write_denies(&mut fake, &grants, &pool());
    assert_eq!(done.skipped, [device]);
    assert_eq!(
        done.failed,
        [(low, ApplyFailure::Io(io::ErrorKind::NotFound))]
    );
    assert_eq!(done.added, on(&paths[0], &grants));
    assert_eq!(fake.adds, on(&paths[0], &grants));
}

#[test]
fn a_failed_add_rolls_back_only_its_own_target_and_the_rest_carry_on() {
    let (_dir, paths) = folders("shared-add-fail", &["a", "u/AppData/LocalLow", "c"]);
    let grants = plan(&paths);
    // Adds in order: a's group, LocalLow's group, LocalLow's owner (the third), c's group.
    let mut fake = Fake {
        fail_at: Some(3),
        ..Fake::default()
    };
    let done = apply_shared_write_denies(&mut fake, &grants, &pool());
    assert_eq!(fake.adds, grants);
    assert_eq!(
        done.failed,
        [(
            paths[1].clone(),
            ApplyFailure::Io(io::ErrorKind::PermissionDenied)
        )]
    );
    // LocalLow's own group deny was written, so it is the one removed; a's and c's stay.
    assert_eq!(fake.removes, on(&paths[1], &grants)[..1]);
    assert_eq!(done.added, [grants[0].clone(), grants[3].clone()]);
}

#[test]
fn a_failed_walk_keeps_its_target_and_the_rest_carry_on() {
    let (_dir, paths) = folders("shared-walk-fail", &["a", "b", "c"]);
    let grants = plan(&paths);
    // Walks in order: a, then b (the second), which fails.
    let mut fake = Fake {
        fail_walk_at: Some(2),
        ..Fake::default()
    };
    let done = apply_shared_write_denies(&mut fake, &grants, &pool());
    assert_eq!(
        done.failed,
        [(
            paths[1].clone(),
            ApplyFailure::Io(io::ErrorKind::PermissionDenied)
        )]
    );
    // b's deny stays: nothing is removed, and all three grants are listed as written.
    assert_eq!(fake.removes, Vec::<Grant>::new());
    assert_eq!(fake.walks.len(), 3);
    assert_eq!(done.added, grants);
}

#[test]
fn a_walk_repairs_the_children_a_pool_user_owns() {
    let (_dir, paths) = folders("shared-repair", &["a"]);
    let grants = plan(&paths);
    let mut fake = Fake::default();
    apply_shared_write_denies(&mut fake, &grants, &pool());
    // Repair mode: every pool user is an owner whose children are repaired, not refused, and the
    // group and the pool users are watched for explicit allows.
    let (_, _, strict) = &fake.walks[0];
    let strict = strict.as_ref().unwrap();
    assert_eq!(strict.pool, pool());
    assert_eq!(
        strict.watched,
        [group(), sid(USER_SID), sid(OTHER_POOL_SIDS[0])]
    );
}

#[test]
fn every_walk_is_strict_with_the_group_and_every_pool_user() {
    let (_dir, paths) = folders("shared-strict", &["a", "u/AppData/LocalLow"]);
    let grants = plan(&paths);
    let mut fake = Fake::default();
    apply_shared_write_denies(&mut fake, &grants, &pool());
    let want = Strict {
        watched: vec![group(), sid(USER_SID), sid(OTHER_POOL_SIDS[0])],
        pool: pool(),
    };
    assert_eq!(fake.walks.len(), 3);
    for (_, reach, strict) in &fake.walks {
        assert_eq!(*reach, Reach::Tree);
        assert_eq!(strict, &Some(want.clone()));
    }
}

#[test]
fn an_entry_already_there_is_walked_but_not_listed_and_not_undone() {
    let (_dir, paths) = folders("shared-already", &["a", "b", "c"]);
    let grants = plan(&paths);
    let mut fake = Fake {
        already_at: Some(1),
        fail_at: Some(3),
        ..Fake::default()
    };
    let done = apply_shared_write_denies(&mut fake, &grants, &pool());
    // a was there already: walked, not listed. b was written and stays. c's add fails and it
    // has nothing of its own to undo.
    assert_eq!(fake.walks.len(), 2);
    assert_eq!(fake.walks[0].0, paths[0]);
    assert_eq!(done.added, [grants[1].clone()]);
    assert_eq!(fake.removes, Vec::<Grant>::new());
    assert_eq!(
        done.failed,
        [(
            paths[2].clone(),
            ApplyFailure::Io(io::ErrorKind::PermissionDenied)
        )]
    );
}

#[test]
fn the_walks_skipped_files_are_counted() {
    let (_dir, paths) = folders("shared-skips", &["a", "u/AppData/LocalLow"]);
    let grants = plan(&paths);
    let mut fake = Fake {
        walk_skips: 1,
        ..Fake::default()
    };
    let done = apply_shared_write_denies(&mut fake, &grants, &pool());
    // Three walks (a, and LocalLow's two grants), one skipped file each.
    assert_eq!(done.walk_skipped, 3);
}

#[test]
fn a_successful_apply_lists_each_written_grant_and_removes_nothing() {
    let (_dir, paths) = folders("shared-ok", &["a", "u/AppData/LocalLow"]);
    let grants = plan(&paths);
    let mut fake = Fake::default();
    let done = apply_shared_write_denies(&mut fake, &grants, &pool());
    assert_eq!(done.added, grants);
    assert_eq!(done.skipped, Vec::<PathBuf>::new());
    assert_eq!(done.failed, Vec::<(PathBuf, ApplyFailure)>::new());
    assert_eq!(fake.removes, Vec::<Grant>::new());
}

#[test]
fn remove_removes_and_walks_each_existing_grant_in_order() {
    let (_dir, paths) = folders("shared-remove", &["a", "u/AppData/LocalLow"]);
    let grants = plan(&paths);
    let mut fake = Fake::default();
    remove_shared_write_denies(&mut fake, &grants).unwrap();
    assert_eq!(fake.removes, grants);
    assert_eq!(fake.adds, Vec::<Grant>::new());
    let walked: Vec<_> = fake.walks.iter().map(|(root, ..)| root.clone()).collect();
    let roots: Vec<_> = grants.iter().map(|g| g.path().to_owned()).collect();
    assert_eq!(walked, roots);
    assert!(fake.walks.iter().all(|(_, _, strict)| strict.is_none()));
}

#[test]
fn remove_ignores_a_missing_target() {
    let (dir, paths) = folders("shared-remove-missing", &["a"]);
    let missing = dir.path().join("absent");
    let grants = plan(&[missing, paths[0].clone()]);
    let mut fake = Fake::default();
    remove_shared_write_denies(&mut fake, &grants).unwrap();
    assert_eq!(fake.removes, [grants[1].clone()]);
    assert_eq!(fake.walks.len(), 1);
}

#[cfg(windows)]
mod ntfs {
    use super::*;
    use crate::windows::NtfsAcl;
    use crate::windows::acl::descriptor;
    use crate::windows::acl_bytes::INHERITED;
    use crate::windows::acl_bytes::owner_sid;
    use crate::windows::test_acl::{Place, entries, grant, has};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// `ACCESS_DENIED_ACE_TYPE`.
    const DENIED: u8 = 1;

    /// The entries on `path` for `subject`, as (type, flags, mask).
    fn subject_entries(path: &Path, subject: &str) -> Vec<(u8, u8, u32)> {
        entries(path)
            .into_iter()
            .filter(|(_, _, _, sid)| sid.as_str() == subject)
            .map(|(kind, flags, mask, _)| (kind, flags, mask))
            .collect()
    }

    /// Asserts `path` has one deny for `subject`, with the full mask, and whether it's inherited.
    fn assert_deny(path: &Path, subject: &str, inherited: bool) {
        let found = subject_entries(path, subject);
        assert_eq!(found.len(), 1, "{}: {found:?}", path.display());
        let (kind, flags, mask) = found[0];
        assert_eq!(kind, DENIED, "{}", path.display());
        assert_eq!(mask, 0x000d_0156, "{}", path.display());
        assert_eq!(
            flags & INHERITED != 0,
            inherited,
            "{}: flags {flags:#x}",
            path.display()
        );
    }

    /// Whether this process's token is elevated. A token that cannot be asked counts as not.
    #[expect(unsafe_code, reason = "FFI to read this process's token elevation")]
    fn elevated() -> bool {
        let mut token = HANDLE::default();
        // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `token` is a
        // live out-pointer for the call.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }.is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut needed = 0u32;
        let len = u32::try_from(size_of::<TOKEN_ELEVATION>()).unwrap_or_default();
        // SAFETY: `elevation` is live and `len` bytes long for the call, which writes a
        // TOKEN_ELEVATION into it; the handle came from OpenProcessToken.
        let asked = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&raw mut elevation).cast()),
                len,
                &raw mut needed,
            )
        };
        // SAFETY: the handle is closed once, here; a failed close leaves nothing to do.
        drop(unsafe { CloseHandle(token) });
        asked.is_ok() && elevation.TokenIsElevated != 0
    }

    #[test]
    fn a_child_a_pool_user_owns_is_repaired_and_gets_the_inherited_denies() {
        // The inherited OWNER RIGHTS deny blocks WRITE_DAC for a child's owner, and here the
        // owner is this user, who also runs the walk. In production the elevated admin who
        // sets up is never the owner of a pool user's children, so the deny does not bind it.
        // Unelevated, the repair cannot run, so the test skips.
        if !elevated() {
            eprintln!("skipped: the repair test needs an elevated token");
            return;
        }
        let place = Place::new("shared-repair-ntfs");
        place
            .dir
            .file("AppData/LocalLow/Microsoft/Sub/doc.txt", "made");
        let target = place.at("AppData/LocalLow");
        let child = place.at("AppData/LocalLow/Microsoft/Sub");
        // The test's own user stands in for the pool user: it owns the child, and an explicit
        // allow of its own sits on the child, as a file the agent made carries.
        let pool_user = owner_sid(&descriptor(&child).unwrap()).unwrap();
        let mut acl = NtfsAcl::new().unwrap();
        acl.add(&grant(
            &child,
            pool_user.as_str(),
            Rights::Modify,
            Inherit::Tree,
        ))
        .unwrap();
        assert!(has(&child, pool_user.as_str(), false));

        let grants = plan(std::slice::from_ref(&target));
        let done = apply_shared_write_denies(&mut acl, &grants, std::slice::from_ref(&pool_user));
        assert_eq!(done.failed, Vec::<(PathBuf, ApplyFailure)>::new());
        // The repair strips the explicit allow, and the walk gives the child both denies.
        assert!(!has(&child, pool_user.as_str(), false));
        assert_deny(&child, GROUP_SID, true);
        assert_deny(&child, OWNER_RIGHTS_SID, true);
    }

    #[test]
    fn a_real_local_low_gets_both_denies_and_what_was_there_inherits_them() {
        let place = Place::new("shared-ntfs");
        place
            .dir
            .file("AppData/LocalLow/Microsoft/Sub/doc.txt", "old");
        let target = place.at("AppData/LocalLow");
        let child = place.at("AppData/LocalLow/Microsoft/Sub");
        let file = place.at("AppData/LocalLow/Microsoft/Sub/doc.txt");
        let grants = plan(std::slice::from_ref(&target));
        let mut acl = NtfsAcl::new().unwrap();

        let done = apply_shared_write_denies(&mut acl, &grants, &[sid(USER_SID)]);
        assert_eq!(done.failed, Vec::<(PathBuf, ApplyFailure)>::new());
        assert_eq!(done.skipped, Vec::<PathBuf>::new());
        assert_eq!(done.added.len(), 2);
        let (group_sid, owner_sid) = (GROUP_SID, OWNER_RIGHTS_SID);
        assert_deny(&target, group_sid, false);
        assert_deny(&target, owner_sid, false);
        // Inherit-only: the folder's own entry is OI|CI|IO, so SYSTEM's rights on LocalLow stay.
        assert_eq!(
            subject_entries(&target, owner_sid),
            [(DENIED, 0x0b, 0x000d_0156)]
        );
        // The folder and the file were there before the apply: the walk gives them both denies.
        // A child folder's inherited copy applies to the child itself, not as inherit-only.
        assert_eq!(subject_entries(&child, owner_sid)[0].1 & 0x8, 0);
        for path in [&child, &file] {
            assert_deny(path, group_sid, true);
            assert_deny(path, owner_sid, true);
        }

        remove_shared_write_denies(&mut acl, &grants).unwrap();
        for path in [&target, &child, &file] {
            assert!(
                subject_entries(path, group_sid).is_empty(),
                "{}",
                path.display()
            );
            assert!(
                subject_entries(path, owner_sid).is_empty(),
                "{}",
                path.display()
            );
        }
    }
}

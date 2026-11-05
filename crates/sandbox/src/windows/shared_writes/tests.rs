//! The shared places' plan, apply and removal against a fake writer, on every OS. The real-NTFS
//! round trip is at the end, for Windows only.

use std::io;
use std::path::{Path, PathBuf};

use super::*;
use crate::test_support::TempDir;
use crate::windows::acl_plan::Sid;
use crate::windows::writer::{Label, Reach, Walked};

const GROUP: &str = "S-1-5-21-1111111111-2222222222-3333333333-1010";

/// Records each call. The `fail_at`-th add (1-based) fails; the `already_at`-th add answers
/// [`Added::AlreadyThere`]; the `fail_walk_at`-th walk fails. A walk is recorded as its root, its
/// reach, and whether it had no strict SIDs.
#[derive(Default)]
struct Fake {
    adds: Vec<Grant>,
    removes: Vec<Grant>,
    walks: Vec<(PathBuf, Reach, bool)>,
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
        strict: Option<&[Sid]>,
    ) -> Result<Walked, ApplyFailure> {
        self.walks.push((root.to_owned(), reach, strict.is_none()));
        if self.fail_walk_at == Some(self.walks.len()) {
            return Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied));
        }
        Ok(Walked::default())
    }

    fn label(&mut self, _path: &Path) -> io::Result<Option<Label>> {
        Ok(None)
    }

    fn restore_label(&mut self, _grant: &Grant, _label: Label) -> Result<(), ApplyFailure> {
        Ok(())
    }
}

fn group() -> Sid {
    Sid::parse(GROUP).unwrap()
}

/// A temp folder holding `names` as folders, and the paths of those folders.
fn folders(test: &str, names: &[&str]) -> (TempDir, Vec<PathBuf>) {
    let dir = TempDir::new(test);
    let paths = names.iter().map(|name| dir.dir(name)).collect();
    (dir, paths)
}

#[test]
fn the_targets_are_device_sync_then_each_pool_profiles_local_low_in_order() {
    let program_data = Path::new("pd");
    let profiles = [PathBuf::from("users/a"), PathBuf::from("users/b")];
    let targets = shared_write_targets(program_data, &profiles);
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
fn each_planned_grant_is_a_deny_write_for_the_group_on_its_target_as_a_tree() {
    let targets = [
        PathBuf::from("pd/Microsoft/DeviceSync"),
        PathBuf::from("u/AppData/LocalLow"),
    ];
    let grants = plan_shared_write_denies(&group(), &targets);
    assert_eq!(grants.len(), 2);
    for (grant, target) in grants.iter().zip(&targets) {
        assert_eq!(grant.path(), target.as_path());
        assert_eq!(grant.sid(), &group());
        assert_eq!(grant.rights(), Rights::DenyWrite);
        assert_eq!(grant.rights().mask(), 0x000d_0156);
        assert_eq!(grant.inherit(), Inherit::Tree);
        assert_eq!(grant.kind(), PathKind::SharedWrite);
    }
}

#[test]
fn a_missing_target_is_skipped_and_listed_and_the_rest_are_written() {
    let (_dir, paths) = folders("shared-missing", &["device", "low"]);
    let missing = paths[0].with_file_name("absent");
    let targets = [paths[0].clone(), missing.clone(), paths[1].clone()];
    let grants = plan_shared_write_denies(&group(), &targets);
    let mut fake = Fake::default();
    let done = apply_shared_write_denies(&mut fake, &grants).unwrap();
    assert_eq!(done.skipped, [missing]);
    assert_eq!(done.added, [grants[0].clone(), grants[2].clone()]);
    assert_eq!(fake.adds, [grants[0].clone(), grants[2].clone()]);
    assert_eq!(fake.removes, Vec::<Grant>::new());
    let walked: Vec<_> = fake.walks.iter().map(|(root, ..)| root.clone()).collect();
    assert_eq!(walked, [paths[0].clone(), paths[1].clone()]);
}

#[test]
fn a_failed_second_add_removes_the_first_and_returns_the_error() {
    let (_dir, paths) = folders("shared-fail", &["device", "low"]);
    let grants = plan_shared_write_denies(&group(), &paths);
    let mut fake = Fake {
        fail_at: Some(2),
        ..Fake::default()
    };
    let result = apply_shared_write_denies(&mut fake, &grants);
    assert_eq!(
        result,
        Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied))
    );
    assert_eq!(fake.adds, grants);
    assert_eq!(fake.removes, [grants[0].clone()]);
}

#[test]
fn an_entry_already_there_is_not_listed_and_not_rolled_back() {
    let (_dir, paths) = folders("shared-already", &["a", "b", "c"]);
    let grants = plan_shared_write_denies(&group(), &paths);
    let mut fake = Fake {
        already_at: Some(1),
        fail_at: Some(3),
        ..Fake::default()
    };
    assert!(apply_shared_write_denies(&mut fake, &grants).is_err());
    // The first was there already, so only the second (written) is removed.
    assert_eq!(fake.removes, [grants[1].clone()]);
}

#[test]
fn each_add_is_followed_by_a_tree_walk_with_no_strict_sids_even_when_already_there() {
    let (_dir, paths) = folders("shared-walks", &["a", "b"]);
    let grants = plan_shared_write_denies(&group(), &paths);
    let mut fake = Fake {
        already_at: Some(1),
        ..Fake::default()
    };
    let done = apply_shared_write_denies(&mut fake, &grants).unwrap();
    assert_eq!(done.added, [grants[1].clone()]);
    assert_eq!(
        fake.walks,
        [
            (paths[0].clone(), Reach::Tree, true),
            (paths[1].clone(), Reach::Tree, true),
        ]
    );
}

#[test]
fn a_failed_walk_rolls_back_the_entry_it_followed_and_the_earlier_ones() {
    let (_dir, paths) = folders("shared-walk-fail", &["a", "b", "c"]);
    let grants = plan_shared_write_denies(&group(), &paths);
    let mut fake = Fake {
        fail_walk_at: Some(2),
        ..Fake::default()
    };
    let result = apply_shared_write_denies(&mut fake, &grants);
    assert_eq!(
        result,
        Err(ApplyFailure::Io(io::ErrorKind::PermissionDenied))
    );
    // The second grant was written before its walk failed, so both come back, last first.
    assert_eq!(fake.adds, [grants[0].clone(), grants[1].clone()]);
    assert_eq!(fake.removes, [grants[1].clone(), grants[0].clone()]);
    // Two walks for the apply, then each rolled-back entry's walk, last first.
    let roots: Vec<_> = fake.walks.iter().map(|(root, ..)| root.clone()).collect();
    assert_eq!(
        roots,
        [
            paths[0].clone(),
            paths[1].clone(),
            paths[1].clone(),
            paths[0].clone()
        ]
    );
}

#[test]
fn remove_walks_each_existing_target_after_removing_its_entry() {
    let (_dir, paths) = folders("shared-remove-walk", &["a", "b"]);
    let grants = plan_shared_write_denies(&group(), &paths);
    let mut fake = Fake::default();
    remove_shared_write_denies(&mut fake, &grants).unwrap();
    assert_eq!(
        fake.walks,
        [
            (paths[0].clone(), Reach::Tree, true),
            (paths[1].clone(), Reach::Tree, true),
        ]
    );
}

#[test]
fn a_successful_apply_lists_each_written_grant_and_removes_nothing() {
    let (_dir, paths) = folders("shared-ok", &["a", "b"]);
    let grants = plan_shared_write_denies(&group(), &paths);
    let mut fake = Fake::default();
    let done = apply_shared_write_denies(&mut fake, &grants).unwrap();
    assert_eq!(done.added, grants);
    assert_eq!(done.skipped, Vec::<PathBuf>::new());
    assert_eq!(fake.removes, Vec::<Grant>::new());
}

#[test]
fn remove_calls_remove_for_each_existing_target_in_order() {
    let (_dir, paths) = folders("shared-remove", &["a", "b"]);
    let grants = plan_shared_write_denies(&group(), &paths);
    let mut fake = Fake::default();
    remove_shared_write_denies(&mut fake, &grants).unwrap();
    assert_eq!(fake.removes, grants);
    assert_eq!(fake.adds, Vec::<Grant>::new());
}

#[test]
fn remove_ignores_a_missing_target() {
    let (_dir, paths) = folders("shared-remove-missing", &["a"]);
    let missing = paths[0].with_file_name("absent");
    let grants = plan_shared_write_denies(&group(), &[missing, paths[0].clone()]);
    let mut fake = Fake::default();
    remove_shared_write_denies(&mut fake, &grants).unwrap();
    assert_eq!(fake.removes, [grants[1].clone()]);
    assert_eq!(fake.walks.len(), 1);
}

#[cfg(windows)]
mod ntfs {
    use super::*;
    use crate::windows::NtfsAcl;
    use crate::windows::acl_bytes::INHERITED;
    use crate::windows::test_acl::{Place, entries};
    use crate::windows::test_world::GROUP_SID;

    /// `ACCESS_DENIED_ACE_TYPE`.
    const DENIED: u8 = 1;
    /// `OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE`.
    const TREE: u8 = 0x3;

    /// The entries on `path` for the pool group, as (type, flags, mask).
    fn group_entries(path: &Path) -> Vec<(u8, u8, u32)> {
        entries(path)
            .into_iter()
            .filter(|(_, _, _, sid)| sid.as_str() == GROUP_SID)
            .map(|(kind, flags, mask, _)| (kind, flags, mask))
            .collect()
    }

    #[test]
    fn a_real_tree_gets_the_deny_on_its_folder_and_inherited_on_what_was_already_there() {
        let place = Place::new("shared-ntfs");
        place.dir.file("LocalLow/Microsoft/Sub/doc.txt", "old");
        let target = place.at("LocalLow");
        let child = place.at("LocalLow/Microsoft/Sub");
        let file = place.at("LocalLow/Microsoft/Sub/doc.txt");
        let grants = plan_shared_write_denies(
            &Sid::parse(GROUP_SID).unwrap(),
            std::slice::from_ref(&target),
        );
        let mut acl = NtfsAcl::new().unwrap();

        let done = apply_shared_write_denies(&mut acl, &grants).unwrap();
        assert_eq!(done.added.len(), 1);
        let on_target = group_entries(&target);
        assert_eq!(on_target.len(), 1, "{on_target:?}");
        let (kind, flags, mask) = on_target[0];
        assert_eq!(kind, DENIED);
        assert_eq!(mask, 0x000d_0156);
        assert_eq!(flags & TREE, TREE, "flags {flags:#x}");

        // The folder and the file were there before the apply: the walk gives them the deny.
        for path in [&child, &file] {
            let inherited = group_entries(path);
            assert_eq!(inherited.len(), 1, "{}: {inherited:?}", path.display());
            let (kind, flags, mask) = inherited[0];
            assert_eq!(kind, DENIED);
            assert_eq!(mask, 0x000d_0156);
            assert_ne!(flags & INHERITED, 0, "{}: flags {flags:#x}", path.display());
        }

        remove_shared_write_denies(&mut acl, &grants).unwrap();
        for path in [&target, &child, &file] {
            assert!(group_entries(path).is_empty(), "{}", path.display());
        }
    }
}

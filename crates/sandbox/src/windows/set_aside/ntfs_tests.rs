//! The set-aside on NTFS, through the public calls: real file IDs, junctions planted on the way
//! to the set-aside folder, restores through the real renames, and round 3's scan: names as NTFS
//! matches them, a folder the lawyer can't list, and the exit check after a repair that refused
//! (R1). Windows only; they run on the VM.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::time::timestamp;
use super::{Reason, SetAsideFailure, after_session, before_launch, list_set_aside};
use super::{restore_set_aside, set_aside_path};
use crate::Error;
use crate::windows::test_junction::junction;
use crate::windows::test_world::{World, matter_one};
use crate::windows::write::rename_new;

mod round3;
mod round4;

fn launch() -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(1_762_337_700_123)
}

fn exit() -> SystemTime {
    launch() + Duration::from_secs(3600)
}

/// The matter's set-aside folder under `data`.
fn base(data: &Path) -> PathBuf {
    data.join("sandbox/set-aside")
        .join(matter_one().to_string())
}

fn is_empty(folder: &Path) -> bool {
    std::fs::read_dir(folder).unwrap().next().is_none()
}

#[test]
fn a_lawyer_folder_renamed_to_git_is_set_aside_by_its_file_id() {
    let w = World::new("ntfs-aside-rename");
    w.dir
        .file("matter/Precedents/hooks/pre-commit", "exit 0\r\n");
    w.dir.file("matter/CLAUDE.md", "# the lawyer's\n");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();

    let root = policy.matter();
    rename_new(&root.join("Precedents"), &root.join(".git")).unwrap();
    let moved = after_session(&policy, &data, &matter_one(), "s1", exit())
        .unwrap()
        .moved;

    let names: Vec<_> = moved
        .iter()
        .map(|e| (e.name.to_str().unwrap(), e.reason))
        .collect();
    assert_eq!(names, [(".git", Reason::New)], "CLAUDE.md kept its file ID");
    assert!(moved[0].to.join("hooks/pre-commit").is_file());
    assert!(!root.join(".git").exists());
    assert!(root.join("CLAUDE.md").is_file());
    assert!(
        moved[0]
            .owner
            .as_deref()
            .is_some_and(|s| s.starts_with("S-1-"))
    );
}

#[test]
fn a_junction_named_git_is_moved_as_itself() {
    let w = World::new("ntfs-aside-junction-item");
    let target = w.dir.file("elsewhere/Precedents/keep.txt", "kept");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();

    junction(
        &policy.matter().join(".git"),
        &w.canonical("elsewhere/Precedents"),
    )
    .unwrap();
    let moved = after_session(&policy, &data, &matter_one(), "s1", exit())
        .unwrap()
        .moved;

    assert_eq!(moved.len(), 1);
    assert!(std::fs::symlink_metadata(&moved[0].to).is_ok());
    assert!(target.is_file(), "what the junction names is untouched");
    assert!(!policy.matter().join(".git").exists());
}

#[test]
fn a_junction_at_the_pass_folder_is_refused_and_nothing_moves_through_it() {
    let w = World::new("ntfs-aside-junction-pass");
    let decoy = w.dir.dir("elsewhere/decoy");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();
    w.dir.file("matter/.mcp.json", "{}");

    junction(&base(&data).join(timestamp(exit())), &decoy).unwrap();
    let error = after_session(&policy, &data, &matter_one(), "s1", exit()).unwrap_err();

    assert!(
        matches!(
            error,
            Error::SetAside {
                failure: SetAsideFailure::Changed,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(policy.matter().join(".mcp.json").is_file());
    assert!(is_empty(&decoy));
}

#[test]
fn a_junction_at_the_set_aside_folder_is_refused_and_nothing_moves_through_it() {
    for part in ["set-aside", "set-aside/m"] {
        let w = World::new("ntfs-aside-junction-folder");
        let decoy = w.dir.dir("elsewhere/decoy");
        let policy = w.builder().build().unwrap();
        let data = w.canonical("data");
        before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();
        w.dir.file("matter/.mcp.json", "{}");

        let planted = if part == "set-aside" {
            data.join("sandbox/set-aside")
        } else {
            base(&data)
        };
        rename_new(&planted, &data.join("sandbox/was")).unwrap();
        junction(&planted, &decoy).unwrap();
        let error = after_session(&policy, &data, &matter_one(), "s1", exit()).unwrap_err();

        assert!(matches!(error, Error::SetAside { .. }), "{part}: {error:?}");
        assert!(policy.matter().join(".mcp.json").is_file(), "{part}");
        assert!(is_empty(&decoy), "{part}");
    }
}

#[test]
fn restore_moves_back_through_the_real_renames_and_never_overwrites() {
    let w = World::new("ntfs-aside-restore");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();
    w.dir
        .file("matter/.claude/settings.json", "{\"agent\": true}");
    let entry = after_session(&policy, &data, &matter_one(), "s1", exit())
        .unwrap()
        .moved
        .remove(0);

    let root = policy.matter();
    w.dir
        .file("matter/.claude/settings.json", "{\"lawyer\": true}");
    assert_eq!(
        restore_set_aside(&data, &entry),
        Err(SetAsideFailure::NameTaken)
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        "{\"lawyer\": true}"
    );

    rename_new(&root.join(".claude"), &root.join("lawyer-claude")).unwrap();
    restore_set_aside(&data, &entry).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        "{\"agent\": true}"
    );
    assert_eq!(list_set_aside(&data, &matter_one()).unwrap(), Vec::new());
    assert_eq!(restore_set_aside(&data, &entry), Err(SetAsideFailure::Gone));
}

#[test]
fn the_walks_item_is_set_aside_and_restored() {
    let w = World::new("ntfs-aside-walk");
    w.dir.file("matter/sub/locked.txt", "shut out");
    let data = w.canonical("data");
    let path = w.canonical("matter/sub/locked.txt");
    let entry =
        set_aside_path(&data, &matter_one(), &path, Reason::LawyerCantOpen, exit()).unwrap();
    assert!(!path.exists());
    assert!(entry.to.is_file());
    restore_set_aside(&data, &entry).unwrap();
    assert!(path.is_file());
}

/// N1: a lawyer folder renamed to `sub\.git` after launch is set aside, its hooks with it, at
/// its own path in the pass's folder, through the real NTFS calls and file IDs; the
/// rename itself is `NtSetInformationFile` with a `RootDirectory`, which the VM showed
/// `SetFileInformationByHandle` refuses.
#[test]
fn a_lawyer_folder_renamed_to_a_nested_git_is_set_aside_by_its_file_id() {
    let w = World::new("ntfs-aside-nested");
    w.dir
        .file("matter/Precedents/hooks/pre-commit", "exit 0\r\n");
    w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/other/.mcp.json", "{}");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    let root = policy.matter().to_owned();
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();

    rename_new(&root.join("sub/.git"), &root.join("sub/old-git")).unwrap();
    rename_new(&root.join("Precedents"), &root.join("sub/.git")).unwrap();
    let moved = after_session(&policy, &data, &matter_one(), "s1", exit())
        .unwrap()
        .moved;

    let seen: Vec<_> = moved.iter().map(|e| (e.from.clone(), e.reason)).collect();
    assert_eq!(seen, [(root.join("sub").join(".git"), Reason::Changed)]);
    assert_eq!(
        moved[0].to,
        base(&data).join(timestamp(exit())).join("sub").join(".git")
    );
    assert!(moved[0].to.join("hooks/pre-commit").is_file());
    assert!(root.join("sub/old-git/HEAD").is_file());
    assert!(root.join("other/.mcp.json").is_file(), "unchanged");
    restore_set_aside(&data, &moved[0]).unwrap();
    assert!(root.join("sub/.git/hooks/pre-commit").is_file());
}

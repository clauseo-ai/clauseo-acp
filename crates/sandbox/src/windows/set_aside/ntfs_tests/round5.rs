//! PR #789's review on NTFS: the owner the set-aside lists is read from the handle its move
//! renamed, and a `commondir` is read relative to its folder's handle, as itself, bounded, with
//! `FILE_COMPLETE_IF_OPLOCKED` (an oplock's break under way is a VM row: it needs another
//! process holding one).

use std::ffi::OsStr;

use super::super::ntfs::Ntfs;
use super::super::run::Volume as _;
use super::{after_session, before_launch, exit, launch};
use crate::windows::record::current_user_sid;
use crate::windows::test_junction::junction;
use crate::windows::test_world::{World, matter_one};
use crate::windows::write::rename_new;

#[test]
fn a_set_aside_items_owner_is_read_from_the_handle_its_move_renamed() {
    let w = World::new("ntfs-aside-owner-handle");
    w.dir.file("matter/Precedents/memo.md", "# the lawyer's\n");
    let policy = w.builder().build().unwrap();
    let data = w.canonical("data");
    before_launch(&policy, &data, &matter_one(), "s1", &[], launch()).unwrap();

    let root = policy.matter();
    rename_new(&root.join("Precedents"), &root.join(".git")).unwrap();
    let moved = after_session(&policy, &data, &matter_one(), "s1", exit())
        .unwrap()
        .moved;
    let lawyer = current_user_sid().unwrap().to_string();
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].owner.as_deref(), Some(lawyer.as_str()));
}

#[test]
fn a_commondir_is_read_through_its_folders_handle_bounded_and_never_through_a_link() {
    let w = World::new("ntfs-aside-read-small");
    w.dir.file("wt/commondir", "../common\r\n");
    w.dir.dir("wt/folder");
    w.dir.dir("elsewhere");
    let wt = w.canonical("wt");
    junction(&wt.join("linked"), &w.canonical("elsewhere")).unwrap();
    let folder = Ntfs.open_root(&wt).unwrap();

    let read = |name: &str, max| Ntfs.read_small(&folder, OsStr::new(name), max);
    assert_eq!(read("commondir", 4096).unwrap(), b"../common\r\n");
    assert_eq!(read("commondir", 3).unwrap(), b"../");
    assert!(read("folder", 4096).is_err(), "a folder isn't read");
    assert!(read("linked", 4096).is_err(), "a link isn't followed");
    assert!(read("missing", 4096).is_err());
}

//! The change journal on NTFS through [`NtfsJournal`], on real folders in a temp folder: the
//! unprivileged read at this token, the records a new file, a rename in and a new DACL leave,
//! the root's ID against the tree's, the tree's names and junctions, and a folder found by its
//! ID. Each test skips, saying why, on a volume that isn't NTFS or has no journal. Windows only;
//! they run on the VM.

use std::path::Path;

use super::{Change, Flow, Journal, JournalError, JournalState, Listed, NtfsJournal, Usn};
use crate::test_support::remove_dir;
use crate::windows::test_acl::Place;
use crate::windows::test_dacl::auto_inherit_with;
use crate::windows::test_junction::junction;
use crate::windows::write::rename_new;

/// `USN_REASON_FILE_CREATE`.
const FILE_CREATE: u32 = 0x100;
/// `USN_REASON_SECURITY_CHANGE`.
const SECURITY_CHANGE: u32 = 0x800;

/// The journal of the volume holding `root`; `None`, with the reason printed, when the volume
/// isn't NTFS or has no journal.
fn journal(root: &Path) -> Option<JournalState> {
    match NtfsJournal.query(root) {
        Ok(state) => Some(state),
        Err(e @ (JournalError::NotNtfs | JournalError::NoJournal)) => {
            println!("skipped: the temp folder's volume: {e:?}");
            None
        }
        Err(e) => panic!("query: {e:?}"),
    }
}

/// Every record of `root`'s volume from `from` to the journal's end now.
fn records_since(root: &Path, state: JournalState, from: Usn) -> Vec<Change> {
    let to = NtfsJournal.query(root).unwrap().next;
    let mut out = Vec::new();
    NtfsJournal
        .read(root, state.id, from, to, &mut |change| {
            out.push(*change);
            Flow::More
        })
        .unwrap();
    out
}

/// The names in `tree` whose folder is `parent`.
fn listed_under(tree: &[Listed], parent: u64) -> Vec<Listed> {
    tree.iter()
        .copied()
        .filter(|l| l.parent == parent)
        .collect()
}

#[test]
fn the_unprivileged_read_works_from_this_token() {
    let place = Place::new("journal-read");
    let Some(state) = journal(&place.root) else {
        return;
    };
    assert!(state.first <= state.next);
    NtfsJournal
        .read(&place.root, state.id, state.next, state.next, &mut |_| {
            Flow::More
        })
        .unwrap();
}

#[test]
fn a_file_made_after_the_cursor_is_in_the_records() {
    let place = Place::new("journal-made");
    let root = place.dir.dir("root");
    let Some(state) = journal(&root) else {
        return;
    };
    place.dir.file("root/new.txt", "new");
    let facts = NtfsJournal.root(&root).unwrap();
    let tree = NtfsJournal.tree(&root).unwrap();
    let file = listed_under(&tree, facts.id.unwrap())[0].id;
    let records = records_since(&root, state, state.next);
    assert!(
        records
            .iter()
            .any(|c| c.file == file && c.reason & FILE_CREATE != 0),
        "no create record for the new file in {records:?}"
    );
}

#[test]
fn a_file_renamed_in_after_the_cursor_is_in_the_records() {
    let place = Place::new("journal-renamed-in");
    let root = place.dir.dir("root");
    let outside = place.dir.file("outside/in.txt", "from outside");
    let Some(state) = journal(&root) else {
        return;
    };
    rename_new(&outside, &root.join("in.txt")).unwrap();
    let facts = NtfsJournal.root(&root).unwrap();
    let root_id = facts.id.unwrap();
    let tree = NtfsJournal.tree(&root).unwrap();
    let file = listed_under(&tree, root_id)[0].id;
    let records = records_since(&root, state, state.next);
    assert!(
        records
            .iter()
            .any(|c| c.file == file && c.parent == root_id),
        "no record of the file under the root in {records:?}"
    );
}

#[test]
fn a_new_dacl_after_the_cursor_is_in_the_records() {
    let place = Place::new("journal-dacl");
    let root = place.dir.dir("root");
    let path = place.dir.file("root/sub/f.txt", "f");
    let Some(state) = journal(&root) else {
        return;
    };
    auto_inherit_with(&path, "D:(A;;FA;;;WD)").unwrap();
    let facts = NtfsJournal.root(&root).unwrap();
    let tree = NtfsJournal.tree(&root).unwrap();
    let sub = listed_under(&tree, facts.id.unwrap())[0].id;
    let file = listed_under(&tree, sub)[0].id;
    let records = records_since(&root, state, state.next);
    assert!(
        records
            .iter()
            .any(|c| c.file == file && c.reason & SECURITY_CHANGE != 0),
        "no security change for the file in {records:?}"
    );
}

#[test]
fn the_root_id_changes_when_a_folder_replaces_the_root() {
    let place = Place::new("journal-root-replaced");
    let root = place.dir.dir("root");
    let other = place.dir.dir("other");
    if journal(&root).is_none() {
        return;
    }
    let before = NtfsJournal.root(&root).unwrap();
    rename_new(&root, &place.at("away")).unwrap();
    rename_new(&other, &place.at("root")).unwrap();
    let after = NtfsJournal.root(&place.at("root")).unwrap();
    assert!(before.ntfs && after.ntfs);
    assert_eq!(before.volume, after.volume);
    assert!(before.id.is_some() && after.id.is_some());
    assert_ne!(before.id, after.id);
    assert_eq!(before.path, after.path);
}

#[test]
fn root_reads_the_same_id_as_the_tree_lists() {
    let place = Place::new("journal-root-id");
    let root = place.dir.dir("root");
    let Some(state) = journal(&root) else {
        return;
    };
    place.dir.file("root/a.txt", "a");
    place.dir.file("root/b/c.txt", "c");
    let facts = NtfsJournal.root(&root).unwrap();
    let tree = NtfsJournal.tree(&root).unwrap();
    let under_root = listed_under(&tree, facts.id.unwrap());
    assert_eq!(under_root.len(), 2, "a.txt and b under the root: {tree:?}");
    let records = records_since(&root, state, state.next);
    for listed in &tree {
        assert!(
            records
                .iter()
                .any(|c| c.file == listed.id && c.reason & FILE_CREATE != 0),
            "no create record for {listed:?} in {records:?}"
        );
    }
}

#[test]
fn tree_lists_every_name_and_never_enters_a_junction() {
    let place = Place::new("journal-tree");
    let root = place.dir.dir("root");
    place.dir.file("root/a.txt", "a");
    place.dir.file("root/sub/b.txt", "b");
    place.dir.file("root/sub/deep/c.txt", "c");
    place.dir.file("elsewhere/x.txt", "x");
    let elsewhere = place.at("elsewhere");
    junction(&root.join("link"), &elsewhere).unwrap();
    if journal(&root).is_none() {
        return;
    }
    let facts = NtfsJournal.root(&root).unwrap();
    let tree = NtfsJournal.tree(&root).unwrap();
    assert_eq!(
        tree.len(),
        6,
        "a.txt, sub, link, b.txt, deep, c.txt: {tree:?}"
    );
    let top = listed_under(&tree, facts.id.unwrap());
    assert_eq!(top.len(), 3);
    assert_eq!(top.iter().filter(|l| l.folder).count(), 1, "sub alone");
    assert_eq!(tree.iter().filter(|l| l.folder).count(), 2, "sub and deep");
    let far = NtfsJournal.tree(&elsewhere).unwrap();
    assert_eq!(far.len(), 1);
    assert!(!tree.iter().any(|l| l.id == far[0].id), "x.txt was listed");
    for link in top.iter().filter(|l| !l.folder) {
        assert!(listed_under(&tree, link.id).is_empty(), "entered {link:?}");
    }
    let through = NtfsJournal.tree(&root.join("link"));
    assert!(through.is_err(), "a junction as the root: {through:?}");
}

#[test]
fn path_of_finds_a_folder_by_its_id() {
    let place = Place::new("journal-path-of");
    let root = place.dir.dir("root");
    let sub = place.dir.dir("root/sub");
    if journal(&root).is_none() {
        return;
    }
    let facts = NtfsJournal.root(&root).unwrap();
    let id = listed_under(&NtfsJournal.tree(&root).unwrap(), facts.id.unwrap())[0].id;
    let found = NtfsJournal
        .path_of(&root, facts.volume, id)
        .unwrap()
        .unwrap();
    assert_eq!(found.path, NtfsJournal.root(&sub).unwrap().path);
    assert!(!found.link);
    let moved = root.join("moved");
    rename_new(&sub, &moved).unwrap();
    let found = NtfsJournal
        .path_of(&root, facts.volume, id)
        .unwrap()
        .unwrap();
    assert_eq!(found.path, NtfsJournal.root(&moved).unwrap().path);
    remove_dir(&moved);
    assert_eq!(NtfsJournal.path_of(&root, facts.volume, id).unwrap(), None);
}

#[test]
fn a_temp_folder_on_the_fixed_system_disk_is_not_removable() {
    let place = Place::new("journal-fixed");
    if journal(&place.root).is_none() {
        return;
    }
    let facts = NtfsJournal.root(&place.root).unwrap();
    assert!(facts.ntfs);
    assert!(
        !facts.removable,
        "the bus query or the drive type failed at this token: {facts:?}"
    );
}

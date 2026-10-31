//! Trees: ids as git gives them, git entry order, and the round trip.

use super::{file, files_under, git_id, new_store, tree_entry};
use crate::history::{HistoryStore, Oid, TreeFile};
use crate::test_dir::TestDir;

#[test]
fn tree_ids_are_the_ids_git_gives() {
    let dir = TestDir::new("history-tree-id");
    let (_git_dir, mut store) = new_store(&dir);
    assert_eq!(
        store.write_tree(&[]).unwrap().to_hex(),
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    );

    // Git order puts the folder `a` (read as `a/`) between `a.txt` and `a0`.
    let files = [file("a.txt", 1), file("a/b", 2), file("a0", 3)];
    let inner = git_id("tree", &tree_entry("100644", "b", Oid([2; 20])));
    let mut root = tree_entry("100644", "a.txt", Oid([1; 20]));
    root.extend(tree_entry("40000", "a", inner));
    root.extend(tree_entry("100644", "a0", Oid([3; 20])));
    assert_eq!(store.write_tree(&files).unwrap(), git_id("tree", &root));
}

#[test]
fn trees_round_trip_with_nested_folders() {
    let dir = TestDir::new("history-tree-round-trip");
    let (git_dir, mut store) = new_store(&dir);
    let files = [
        file("a.txt", 1),
        file("a/b", 2),
        file("a/c/d.docx", 3),
        file("a/c/e", 4),
        file("a0", 5),
        file("z/y/x/w.pdf", 6),
    ];
    let tree = store.write_tree(&files).unwrap();
    assert_eq!(store.read_tree(tree).unwrap(), files);

    let objects = files_under(&git_dir.join("objects"));
    assert_eq!(store.write_tree(&files).unwrap(), tree);
    assert_eq!(files_under(&git_dir.join("objects")), objects);

    let unsorted = [file("b", 1), file("a", 2)];
    assert!(store.write_tree(&unsorted).is_err());
    let clash = [file("a", 1), file("a.txt", 2), file("a/b", 3)];
    assert!(store.write_tree(&clash).is_err());
}

/// A tree of up to 12 files from a few names in up to three levels, with blobs from 1 to 3:
/// small enough that two trees often share subtrees, and a name is now and then a file in one
/// tree and a folder in the other.
fn random_tree(state: &mut u64) -> Vec<TreeFile> {
    const NAMES: [&str; 4] = ["a", "a.txt", "b", "c0"];
    let mut next = || {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    };
    let mut files: Vec<TreeFile> = Vec::new();
    for _ in 0..(next() % 12) {
        let depth = 1 + next() % 3;
        let parts: Vec<&str> = (0..depth)
            .map(|_| NAMES[usize::try_from(next() % 4).unwrap()])
            .collect();
        let path = parts.join("/");
        // A path that is a folder of another, or holds one, would not make a tree.
        let clash = files.iter().any(|other| {
            other.path == path
                || other.path.starts_with(&format!("{path}/"))
                || path.starts_with(&format!("{}/", other.path))
        });
        if !clash {
            files.push(file(&path, u8::try_from(1 + next() % 3).unwrap()));
        }
    }
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    files
}

#[test]
fn changed_files_are_the_files_the_trees_do_not_share() {
    let dir = TestDir::new("history-tree-changed");
    let (_git_dir, mut store) = new_store(&dir);
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for _ in 0..300 {
        let (a, b) = (random_tree(&mut state), random_tree(&mut state));
        let (a_id, b_id) = (store.write_tree(&a).unwrap(), store.write_tree(&b).unwrap());
        let only = |x: &[TreeFile], y: &[TreeFile]| -> Vec<TreeFile> {
            x.iter().filter(|file| !y.contains(file)).cloned().collect()
        };
        let (old, new) = store.changed_files(a_id, b_id).unwrap();
        assert_eq!(
            (&old, &new),
            (&only(&a, &b), &only(&b, &a)),
            "{a:?} -> {b:?}"
        );
        assert_eq!(
            store.changed_files(a_id, a_id).unwrap(),
            (Vec::new(), Vec::new())
        );
    }
}

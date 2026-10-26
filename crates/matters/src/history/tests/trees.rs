//! Trees: ids as git gives them, git entry order, and the round trip.

use super::{file, files_under, git_id, new_store, tree_entry};
use crate::history::{HistoryStore, Oid};
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

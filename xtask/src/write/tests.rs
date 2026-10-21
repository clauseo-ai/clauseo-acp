//! Tests of the folder-confined operations of the write door: each changes only a name directly
//! in the folder it is given.

use super::*;
use crate::scratch::Scratch;

/// Names that are not one plain file name, which every confined operation refuses.
const NOT_PLAIN: [&str; 7] = ["../x", "a/b", "/tmp/x", "", ".", "..", "a\\b"];

#[test]
fn a_file_is_written_directly_in_its_folder_and_nowhere_else() {
    let dir = Scratch::new("write-file-in");
    let folder = dir.join("target/xtask");
    write_file_in(&folder, "mutants.diff", b"one").expect("made with its folders");
    write_file_in(&folder, "mutants.diff", b"two").expect("replaced");
    assert_eq!(
        std::fs::read_to_string(folder.join("mutants.diff")).expect("read it"),
        "two"
    );
    for name in NOT_PLAIN {
        let refused = write_file_in(&dir.join("other"), name, b"x").expect_err("refused");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            refused.to_string(),
            format!("`{name}` is not a file name: it must be one plain file name")
        );
    }
    assert!(!dir.join("other").exists(), "a refused write makes nothing");
}

#[test]
fn only_an_entry_directly_in_the_folder_is_removed() {
    let dir = Scratch::new("write-remove-in");
    let temp = dir.join("tmp");
    Scratch::create_dir_all(temp.join("cargo-mutants-tree-abc.tmp/target")).expect("a tree");
    Scratch::write(temp.join("file"), "x").expect("a file");
    Scratch::create_dir_all(dir.join("kept/inner")).expect("a folder beside tmp");
    for name in NOT_PLAIN
        .iter()
        .copied()
        .chain(["../kept", "cargo-mutants-tree-abc.tmp/target"])
    {
        let refused = remove_in(&temp, name).expect_err("refused");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            refused.to_string(),
            format!("`{name}` is not a name in the folder: it must be one plain file name")
        );
    }
    assert!(dir.join("kept/inner").is_dir(), "nothing outside tmp went");
    assert!(temp.join("cargo-mutants-tree-abc.tmp/target").is_dir());
    remove_in(&temp, "cargo-mutants-tree-abc.tmp").expect("the tree goes");
    remove_in(&temp, "file").expect("the file goes");
    assert!(!temp.join("cargo-mutants-tree-abc.tmp").exists());
    assert!(!temp.join("file").exists());
    let missing = remove_in(&temp, "file").expect_err("a missing entry is an error");
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
}

#[cfg(unix)]
#[test]
fn a_symlink_in_the_folder_is_removed_and_never_followed() {
    let dir = Scratch::new("write-remove-in-link");
    let temp = dir.join("tmp");
    Scratch::create_dir_all(dir.join("target/inner")).expect("a folder outside tmp");
    Scratch::create_dir_all(&temp).expect("tmp");
    Scratch::symlink_dir(dir.join("target"), temp.join("link")).expect("a link out of tmp");
    remove_in(&temp, "link").expect("the link goes");
    assert!(!temp.join("link").exists());
    assert!(
        dir.join("target/inner").is_dir(),
        "what it pointed at stays"
    );
}

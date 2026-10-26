use clauseo_protocol::MatterPath;

use super::{locked, owner_names};
use crate::test_dir::TestDir;
use crate::write;

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

#[test]
fn owner_names_of_a_word_file() {
    assert_eq!(
        owner_names("Contract.docx"),
        ["~$Contract.docx", "~$ntract.docx"]
    );
    assert_eq!(owner_names("ab"), ["~$ab"]);
    assert_eq!(owner_names("éàb"), ["~$éàb", "~$b"]);
}

#[test]
fn files_with_an_owner_file_beside_them_are_locked() {
    let dir = TestDir::new("locks-owner");
    let root = dir.path();
    write::create_dir_all(&root.join("sub")).unwrap();
    for name in [
        "Contract.docx",
        "sub/Brief.docx",
        "Free.docx",
        "~$ntract.docx",
        "sub/~$Brief.docx",
    ] {
        drop(write::create_new(&root.join(name)).unwrap());
    }
    let found = locked(
        root,
        &[
            path("sub/Brief.docx"),
            path("Free.docx"),
            path("Contract.docx"),
            path("Contract.docx"),
            path("Missing.docx"),
        ],
    );
    assert_eq!(found, [path("Contract.docx"), path("sub/Brief.docx")]);
}

#[test]
fn a_missing_file_with_no_owner_file_is_not_locked() {
    let dir = TestDir::new("locks-missing");
    assert_eq!(locked(dir.path(), &[path("gone/x.docx")]), []);
}

//! Windows: a matter path two folders deep under a deep matter folder, and a file another
//! program holds open, refused until its handle is dropped.

use std::fs::File;
use std::io::Write as _;
use std::path::Path;

use clauseo_protocol::MatterPath;

use super::{HeldOpen, held_open, matter_path_in};
use crate::test_dir::TestDir;
use crate::write;

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

/// A file two folders down keeps every folder of its matter path, as an absolute path and as
/// one relative to the matter folder, which sits several folders below the drive.
#[test]
fn a_nested_file_keeps_every_folder_of_its_matter_path() {
    let dir = TestDir::new("locks-win-nested");
    let folder = dir.path().join("m");
    write::create_dir_all(&folder.join("a").join("b")).unwrap();
    let nested = Some(path("a/b/New.docx"));
    assert_eq!(
        matter_path_in(&folder, &folder.join("a").join("b").join("New.docx")),
        nested
    );
    assert_eq!(matter_path_in(&folder, Path::new(r"a\b\New.docx")), nested);
}

/// A file another program has open (this test's own handle, with std's default sharing) is held
/// open by a sharing violation: the check's exclusive open, a second holder, is refused. Once
/// the handle is dropped the file is free, and stays free on a second check, so the check's own
/// open is released too.
#[test]
fn a_file_held_open_is_refused_until_its_handle_is_dropped() {
    let dir = TestDir::new("locks-win-sharing");
    let root = dir.path();
    let name = path("Contract.docx");
    write::create_new(&root.join("Contract.docx"))
        .unwrap()
        .write_all(b"the contract")
        .unwrap();

    let holder = File::open(root.join("Contract.docx")).unwrap();
    assert_eq!(held_open(root, &name), Some(HeldOpen::SharingViolation));
    drop(holder);
    assert_eq!(held_open(root, &name), None);
    assert_eq!(
        held_open(root, &name),
        None,
        "the check kept its own handle"
    );
}

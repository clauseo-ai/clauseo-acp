//! Write-back's listings of folder names, shared by restore and undo: a folder that vanishes
//! while its names are listed holds nothing.

use std::io;
use std::path::Path;

use clauseo_protocol::MatterPath;

use crate::no_download::fault;
use crate::test_dir::TestDir;
use crate::write;
use crate::writeback::Names;

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

fn put(file: &Path) {
    write::create_dir_all(file.parent().unwrap()).unwrap();
    drop(write::create_new(file).unwrap());
}

/// A folder whose listing finds it gone (deleted after the stat of the path in it) holds no
/// entry by that name: the path is not there by its exact bytes, which is no error.
#[test]
fn a_folder_gone_while_listed_holds_nothing() {
    let dir = TestDir::new("writeback-gone-listing");
    put(&dir.path().join("a.txt"));
    fault::fail(dir.path(), 0, io::ErrorKind::NotFound);
    let found = Names::default().metadata(dir.path(), &path("a.txt"));
    fault::clear();
    assert!(matches!(found, Ok(None)), "{found:?}");
}

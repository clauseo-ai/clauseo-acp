//! Write-back's listings of folder names, shared by restore and undo: a folder that vanishes
//! while its names are listed holds nothing. And a restore's last check before it writes: a
//! file and a folder that clash since the plan.

use std::io;
use std::path::Path;

use clauseo_protocol::MatterPath;

use crate::history::Oid;
use crate::no_download::fault;
use crate::restore::{Write, clashed_since};
use crate::test_dir::TestDir;
use crate::writeback::Names;
use crate::{Error, write};

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

/// A folder made where a file goes after the plan checked it, or a file made where its folder
/// goes, refuses the write as the clash it is (`FileFolderClash`, naming the entry in the way),
/// not as a file another program moved (`matter.file_locked`).
#[test]
fn a_clash_made_after_the_plan_is_refused_as_a_clash() {
    let dir = TestDir::new("restore-clash-after-plan");
    write::create_dir_all(&dir.path().join("a.txt")).unwrap();
    put(&dir.path().join("sub"));
    let writing = |text: &str| Write {
        path: path(text),
        blob: Oid([0; 20]),
        now: None,
        size: 1,
    };

    let err = clashed_since(dir.path(), &[writing("new.txt"), writing("a.txt")]).unwrap_err();
    assert!(
        matches!(&err, Error::FileFolderClash { path, in_way }
            if path.as_str() == "a.txt" && in_way.as_str() == "a.txt"),
        "{err:?}"
    );
    let err = clashed_since(dir.path(), &[writing("sub/deep/c.txt")]).unwrap_err();
    assert!(
        matches!(&err, Error::FileFolderClash { path, in_way }
            if path.as_str() == "sub/deep/c.txt" && in_way.as_str() == "sub"),
        "{err:?}"
    );
    assert!(clashed_since(dir.path(), &[writing("new/b.txt")]).is_ok());
}

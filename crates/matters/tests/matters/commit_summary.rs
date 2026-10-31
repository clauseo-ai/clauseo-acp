//! The summary line of each checkpoint's commit (decision record 0007): how many files the
//! checkpoint changed, or for the import how many it holds.

use std::path::Path;

use crate::fixture::{Fixture, created, meta, tools};

/// The first line of the commit the timeline's ref (`refs/clauseo/timeline`) names.
fn tip_summary(git_dir: &Path) -> String {
    use gix::objs::Find as _;
    let hex = std::fs::read_to_string(git_dir.join("refs/clauseo/timeline")).unwrap();
    let id = gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap();
    let odb = gix::odb::at(git_dir.join("objects"), gix::hash::Kind::Sha1).unwrap();
    let mut buffer = Vec::new();
    let data = odb.try_find(&id, &mut buffer).unwrap().unwrap();
    let commit = gix::objs::CommitRef::from_bytes(data.data, gix::hash::Kind::Sha1).unwrap();
    let message = String::from_utf8(commit.message.to_vec()).unwrap();
    message.lines().next().unwrap().to_owned()
}

/// The import's summary counts every file it holds; a later checkpoint's counts only the
/// files it changed.
#[test]
fn summary_counts_the_files_changed() {
    let fx = Fixture::new("summary-count");
    fx.write("a.txt", "a");
    fx.write("b.txt", "b");
    fx.write("c.txt", "c");
    let mut matter = fx.import();
    assert_eq!(tip_summary(&fx.git_dir()), "Import: 3 files");

    fx.write("a.txt", "changed");
    created(matter.after_turn(&meta(2), &tools(&["a.txt"])).unwrap(), 2);
    assert_eq!(tip_summary(&fx.git_dir()), "After turn: 1 file changed");
}

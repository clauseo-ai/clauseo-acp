//! A checkpoint whose publish fails after its ref moved (the stat cache did not write): the
//! matter reads its timeline again and goes on from what is on disk.

#[cfg(unix)]
use clauseo_matters::CompareSide;
use clauseo_matters::Matter;
use clauseo_protocol::ErrorCode;

use crate::fixture::{Fixture, cid, meta};
use crate::support;

/// A matter with `sub/x.txt` imported as "one", then changed to "two" by a before-turn
/// checkpoint (2) whose ref moved but whose stat cache could not be written: `index` is a
/// folder that the new cache cannot replace.
fn failed(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("sub/x.txt", "one");
    let mut matter = fx.import();
    let index = fx.git_dir().join("index");
    support::remove_file(&index);
    support::write(index.join("in-the-way"), "");

    fx.write("sub/x.txt", "two");
    let err = matter.before_turn(&meta(2)).unwrap_err();
    assert_eq!(err.code(), ErrorCode::MatterCheckpointFailed, "{err:?}");
    (fx, matter)
}

/// The checkpoint whose ref moved is on the timeline, as the repository has it.
#[test]
fn the_timeline_holds_the_checkpoint_whose_ref_moved() {
    let (_fx, matter) = failed("failed-publish-timeline");
    let ids: Vec<_> = matter.timeline().iter().map(|entry| entry.id).collect();
    assert_eq!(ids, [cid(1), cid(2)]);
}

/// Comparing that checkpoint with the folder reads the folder against its tree: a file in a
/// folder that does not read keeps the content the checkpoint recorded, so nothing differs.
#[cfg(unix)]
#[test]
fn the_folder_compares_against_the_checkpoint_whose_ref_moved() {
    let (fx, matter) = failed("failed-publish-compare");
    let locked = crate::unreadable::Mode::set(fx.file("sub"), 0o000);
    let changes = matter.compare(CompareSide::Checkpoint(cid(2)), CompareSide::Folder);
    drop(locked);
    assert_eq!(changes.unwrap(), []);
}

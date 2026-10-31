//! Comparing versions and reading them: two checkpoints, a checkpoint and the folder now, the
//! files of a checkpoint and a file's bytes at one.

use clauseo_matters::{CompareSide, Error, VersionFile};
use clauseo_protocol::{ErrorCode, MatterPath};

use crate::fixture::{
    Fixture, added, assert_none, blob, cid, created, deleted, meta, modified, path, read_at,
    renamed, tools,
};
use crate::support;

use CompareSide::{Checkpoint, Folder};

/// Checkpoint 1: `a.txt` "one", `b.txt` "bee", `c.txt` "sea". Checkpoint 3: `a.txt` "two",
/// `b.txt` gone, `c.txt` renamed to `sub/c.txt`, `d.txt` "dee" added. Checkpoint 5: `d.txt`
/// "dee two".
fn three_versions(name: &str) -> (Fixture, clauseo_matters::Matter) {
    let fx = Fixture::new(name);
    fx.write("a.txt", "one");
    fx.write("b.txt", "bee");
    fx.write("c.txt", "sea");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    support::remove_file(fx.file("b.txt"));
    support::create_dir_all(fx.file("sub"));
    support::rename(fx.file("c.txt"), fx.file("sub/c.txt"));
    fx.write("d.txt", "dee");
    let all = tools(&["a.txt", "b.txt", "c.txt", "sub/c.txt", "d.txt"]);
    created(matter.after_turn(&meta(3), &all).unwrap(), 3);
    matter.before_turn(&meta(4)).unwrap();
    fx.write("d.txt", "dee two");
    created(matter.after_turn(&meta(5), &all).unwrap(), 5);
    (fx, matter)
}

#[test]
fn compare_checkpoints_lists_changes() {
    let (_fx, matter) = three_versions("compare-checkpoints");
    assert_eq!(
        matter
            .compare(Checkpoint(cid(1)), Checkpoint(cid(5)))
            .unwrap(),
        [
            modified("a.txt", b"one", b"two"),
            deleted("b.txt", b"bee"),
            added("d.txt", b"dee two"),
            renamed("c.txt", "sub/c.txt", b"sea"),
        ]
    );
    // Swapped, the changes run the other way.
    assert_eq!(
        matter
            .compare(Checkpoint(cid(5)), Checkpoint(cid(1)))
            .unwrap(),
        [
            modified("a.txt", b"two", b"one"),
            added("b.txt", b"bee"),
            renamed("sub/c.txt", "c.txt", b"sea"),
            deleted("d.txt", b"dee two"),
        ]
    );
    assert_eq!(
        matter
            .compare(Checkpoint(cid(3)), Checkpoint(cid(5)))
            .unwrap(),
        [modified("d.txt", b"dee", b"dee two")]
    );
    assert_none(
        &matter
            .compare(Checkpoint(cid(3)), Checkpoint(cid(3)))
            .unwrap(),
    );
    let err = matter
        .compare(Checkpoint(cid(1)), Checkpoint(cid(99)))
        .unwrap_err();
    assert!(
        matches!(&err, Error::UnknownCheckpoint { id } if *id == cid(99)),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::CommandInvalid);
}

#[test]
fn compare_with_folder_sees_unrecorded_edits_and_writes_nothing() {
    let (mut fx, _) = three_versions("compare-folder");
    // Reopened with a small cap: a.txt grows over it, so it is skipped, not deleted.
    fx.limits.file_cap_bytes = 10;
    let matter = fx.open(&fx.folder.clone()).unwrap();
    fx.write("a.txt", "much longer than the cap");
    fx.write("e.txt", "new");
    let index = std::fs::read(fx.git_dir().join("index")).unwrap();

    assert_eq!(
        matter.compare(Checkpoint(cid(5)), Folder).unwrap(),
        [added("e.txt", b"new")]
    );
    assert_eq!(
        matter.compare(Folder, Checkpoint(cid(5))).unwrap(),
        [deleted("e.txt", b"new")]
    );
    assert_none(&matter.compare(Folder, Folder).unwrap());
    assert_eq!(matter.timeline().len(), 3, "no checkpoint taken");
    assert_eq!(std::fs::read(fx.git_dir().join("index")).unwrap(), index);
    let mut bytes = Vec::new();
    assert!(
        matter
            .read_file(cid(5), &path("e.txt"), &mut bytes)
            .is_err(),
        "the folder's new file was not stored as a version"
    );
}

#[test]
fn files_at_and_read_file_return_version_content() {
    let (fx, matter) = three_versions("compare-files-at");
    let file = |p: &str, bytes: &[u8]| VersionFile {
        path: path(p),
        content: blob(bytes),
        size: u64::try_from(bytes.len()).unwrap(),
    };
    assert_eq!(
        matter.files_at(cid(1)).unwrap(),
        [
            file("a.txt", b"one"),
            file("b.txt", b"bee"),
            file("c.txt", b"sea")
        ]
    );
    assert_eq!(
        matter.files_at(cid(5)).unwrap(),
        [
            file("a.txt", b"two"),
            file("d.txt", b"dee two"),
            file("sub/c.txt", b"sea")
        ]
    );
    assert_eq!(read_at(&matter, 1, "b.txt"), b"bee");
    assert_eq!(read_at(&matter, 3, "d.txt"), b"dee");
    let mut out = Vec::new();
    let err = matter
        .read_file(cid(1), &path("d.txt"), &mut out)
        .unwrap_err();
    assert!(
        matches!(&err, Error::UnknownFile { path: p } if *p == path("d.txt")),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::CommandInvalid);
    assert_none(&out);

    // Versions need only the history: they read with the folder gone.
    let moved = fx.dir.path().join("moved");
    support::rename(&fx.folder, &moved);
    assert_eq!(matter.files_at(cid(3)).unwrap().len(), 3);
    assert_eq!(read_at(&matter, 5, "sub/c.txt"), b"sea");
    assert_eq!(
        matter
            .compare(Checkpoint(cid(3)), Checkpoint(cid(5)))
            .unwrap()
            .len(),
        1
    );
    let err = matter.compare(Checkpoint(cid(5)), Folder).unwrap_err();
    assert_eq!(err.code(), ErrorCode::MatterFolderMissing, "{err:?}");
    let chosen: Vec<MatterPath> = vec![path("a.txt")];
    let err = matter
        .preview_restore(cid(1), clauseo_matters::RestoreFiles::Only(&chosen))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::MatterFolderMissing, "{err:?}");
}

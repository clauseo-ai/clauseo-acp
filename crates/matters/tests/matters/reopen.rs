//! Opening a matter again: the timeline from the trailers, a lost stat cache, a moved folder,
//! and the lawyer's own `.git`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clauseo_matters::{CheckpointOutcome, Error, Matter, MissingReason, NewMatter};
use clauseo_protocol::{ErrorCode, FileChange};

use crate::fixture::{Fixture, assert_none, cid, created, matter_id, meta, path, tools, undo};
use crate::support;

#[test]
fn timeline_rebuilds_from_trailers() {
    let fx = Fixture::new("reopen-timeline");
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    fx.write("b.txt", "outside");
    created(matter.before_turn(&meta(2)).unwrap(), 2);
    fx.write("a.txt", "two");
    created(matter.after_turn(&meta(3), &tools(&["a.txt"])).unwrap(), 3);
    assert_eq!(matter.before_turn(&meta(4)).unwrap().outcome.id(), cid(3));
    fx.write("c.txt", "three");
    created(matter.after_turn(&meta(5), &tools(&["c.txt"])).unwrap(), 5);
    let timeline = matter.timeline().to_vec();
    assert_eq!(timeline.len(), 4);
    drop(matter);

    let mut matter = fx.open(&fx.folder).unwrap();
    assert_eq!(matter.timeline(), timeline);
    assert_eq!(matter.last_checkpoint(), Some(cid(5)));
    let undone = undo(&mut matter, (2, 3), &["a.txt"], (6, 7)).unwrap();
    assert_eq!(undone.restored, [path("a.txt")]);
    assert_eq!(fx.read("a.txt"), b"one");
    drop(matter);

    // Without the stat cache every file is read once; nothing changed, so nothing is listed.
    support::remove_file(fx.git_dir().join("index"));
    let mut matter = fx.open(&fx.folder).unwrap();
    assert_eq!(matter.timeline().len(), 5);
    let checkpoint = matter.before_turn(&meta(8)).unwrap();
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(7)));
    assert_none(&checkpoint.changes);
    assert_none(&checkpoint.skipped);
    assert!(fx.git_dir().join("index").is_file());
}

#[test]
fn moved_folder_is_folder_missing() {
    let fx = Fixture::new("moved-folder");
    fx.write("a.txt", "a");
    drop(fx.import());
    let moved = fx.dir.path().join("moved");
    support::rename(&fx.folder, &moved);

    let err = fx.open(&fx.folder).unwrap_err();
    assert!(
        matches!(
            err,
            Error::FolderMissing {
                reason: MissingReason::NoFolder,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::MatterFolderMissing);
    let mut matter = fx.open(&moved).unwrap();
    assert_eq!(matter.folder(), moved);
    assert_eq!(matter.before_turn(&meta(2)).unwrap().outcome.id(), cid(1));

    // Another matter's folder.
    let other = fx.dir.path().join("other");
    let new = NewMatter {
        id: matter_id(2),
        folder: other.clone(),
        checkpoint: cid(100),
        ..fx.new_matter()
    };
    drop(Matter::create(fx.settings(), new).unwrap());
    let err = fx.open(&other).unwrap_err();
    let expected = MissingReason::OtherMatter(matter_id(2));
    assert!(
        matches!(err, Error::FolderMissing { reason, .. } if reason == expected),
        "{err:?}"
    );

    // The folder vanishes while the matter is open.
    support::rename(&moved, fx.dir.path().join("gone"));
    let err = matter.before_turn(&meta(3)).unwrap_err();
    assert!(
        matches!(
            err,
            Error::FolderMissing {
                reason: MissingReason::NoFolder,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::MatterFolderMissing);
}

/// Every file under `dir` with its bytes and modification time.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                let bytes = std::fs::read(entry.path()).unwrap();
                files.insert(entry.path(), (bytes, metadata.modified().unwrap()));
            }
        }
    }
    files
}

fn no_git(changes: &[FileChange]) {
    for change in changes {
        assert!(!change.path().as_str().contains(".git"), "{change:?}");
    }
}

#[test]
fn lawyers_git_untouched() {
    let fx = Fixture::new("lawyers-git");
    fx.write("a.txt", "one");
    fx.write(".git/HEAD", "ref: refs/heads/main\n");
    fx.write(".git/config", "[core]\n\tbare = false\n");
    fx.write(".git/objects/ab/cdef0123", [9; 64]);
    fx.write(
        ".git/refs/heads/main",
        "0123456789abcdef0123456789abcdef01234567\n",
    );
    fx.write("sub/.GIT/odd", "any case is the lawyer's");
    let git = fx.file(".git");
    let before = snapshot(&git);

    let (mut matter, summary) = fx.import_with_summary();
    assert_eq!(summary.files, 1);
    assert_eq!(snapshot(&git), before);

    // The lawyer commits: nothing is seen.
    fx.write(".git/objects/12/3456", [1; 10]);
    let checkpoint = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_none(&checkpoint.skipped);
    let before = snapshot(&git);

    fx.write("a.txt", "two");
    let changes = created(matter.after_turn(&meta(3), &tools(&["a.txt"])).unwrap(), 3);
    no_git(&changes);
    assert_eq!(changes.len(), 1);
    let undone = undo(&mut matter, (1, 3), &["a.txt"], (4, 5)).unwrap();
    no_git(&created(undone.checkpoint, 5));
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(snapshot(&git), before);
    assert_eq!(fx.read("sub/.GIT/odd"), b"any case is the lawyer's");
}

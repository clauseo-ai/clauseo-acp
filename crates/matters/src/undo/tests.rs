//! Tests of [`undo`] with a sync client that puts back a file the undo just deleted
//! (`no_download::resurrect`): the `undo` checkpoint never keeps such a file with the blob it
//! had, and [`Undone::resurrected`] names it.

use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_protocol::{
    ChangeKind, CheckpointId, CheckpointKind, FileChoice, FileDecision, MatterPath,
    SkippedFileReason, Timestamp,
};

use super::undo;
use crate::checkpoint::{Core, Spec};
use crate::history::{GixStore, HistoryStore};
use crate::no_download::{fault, resurrect};
use crate::test_dir::TestDir;
use crate::types::{
    Checkpoint, CheckpointOutcome, Limits, Retention, Settings, TimelineEntry, UndoRequest, Undone,
};
use crate::{PlaceholderProbe, write};

/// Cloud-only for the paths marked; each call is a probe call for
/// [`resurrect::When::AtNextProbe`].
#[derive(Default)]
struct Probe {
    marked: Arc<Mutex<HashSet<PathBuf>>>,
}

impl PlaceholderProbe for Probe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        resurrect::probed();
        self.marked.lock().unwrap().contains(path)
    }
}

/// A matter with `keep.txt` at checkpoint 1 and the step to checkpoint 2 that `change` makes.
struct Setup {
    _dir: TestDir,
    folder: PathBuf,
    settings: Settings,
    marked: Arc<Mutex<HashSet<PathBuf>>>,
    core: Core,
}

impl Setup {
    fn new(name: &str, change: impl FnOnce(&Path)) -> Self {
        fault::clear();
        let dir = TestDir::new(name);
        let folder = dir.path().join("matter");
        write::create_dir_all(&folder).unwrap();
        let root = dir.path().join("root");
        let probe = Probe::default();
        let marked = Arc::clone(&probe.marked);
        let settings = Settings {
            matters_root: root.clone(),
            refused_trees: Vec::new(),
            limits: Limits::DEFAULT,
            retention: Retention::KeepAll,
            probe: Arc::new(probe),
        };
        let git_dir = root.join("m").join("history.git");
        let mut core = Core::load(GixStore::create(&git_dir, &folder).unwrap()).unwrap();
        put(&folder, "keep.txt", b"kept as it is");
        put(&folder, "memo.txt", b"the memo");
        core.take(&folder, &settings, &spec(1)).unwrap();
        change(&folder);
        core.take(&folder, &settings, &spec(2)).unwrap();
        Self {
            _dir: dir,
            folder,
            settings,
            marked,
            core,
        }
    }

    /// Arms `path` to come back after its delete at `when`, cloud-only or downloaded.
    fn bring_back(&self, path: &str, when: resurrect::When, cloud_only: bool) {
        let marked = Arc::clone(&self.marked);
        let mark: resurrect::Mark = cloud_only.then(|| {
            Box::new(move |path: &Path| {
                marked.lock().unwrap().insert(path.to_owned());
            }) as Box<dyn Fn(&Path)>
        });
        resurrect::on_remove(&self.folder.join(path), when, mark);
    }

    /// Undoes the step 1..2, `path` decided `Undo`, as checkpoints 3 (outside edits) and 4.
    fn undo(&mut self, path: &str) -> Undone {
        let decisions = [FileDecision {
            path: mpath(path),
            choice: FileChoice::Undo,
        }];
        let request = UndoRequest {
            before: cid(1),
            after: cid(2),
            decisions: &decisions,
            thread: None,
            turn: None,
            outside_edits: cid(3),
            checkpoint: cid(4),
            at: at(4),
        };
        let undone = undo(&mut self.core, &self.folder, &self.settings, &request).unwrap();
        assert!(
            !resurrect::pending(),
            "a return armed for the walk never came"
        );
        undone
    }

    /// The paths in the tree of the checkpoint `checkpoint` names, created or reused.
    fn tree(&self, checkpoint: &Checkpoint) -> Vec<String> {
        let (CheckpointOutcome::Created(id) | CheckpointOutcome::Reused(id)) = checkpoint.outcome;
        let files = self.core.tree_files(id).unwrap();
        files
            .iter()
            .map(|file| file.path.as_str().to_owned())
            .collect()
    }

    fn exists(&self, path: &str) -> bool {
        self.folder.join(path).exists()
    }
}

fn put(folder: &Path, path: &str, bytes: &[u8]) {
    let abs = folder.join(path);
    write::create_dir_all(abs.parent().unwrap()).unwrap();
    write::replace(&abs, &mut |file| {
        file.write_all(bytes).unwrap();
        Ok(())
    })
    .unwrap();
}

fn mpath(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

fn paths(texts: &[&str]) -> Vec<MatterPath> {
    texts.iter().map(|text| mpath(text)).collect()
}

fn cid(n: u64) -> CheckpointId {
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

fn at(n: u64) -> Timestamp {
    Timestamp::new(1_700_000_000_000 + i64::try_from(n).unwrap() * 1000).unwrap()
}

fn spec(n: u64) -> Spec<'static> {
    Spec {
        entry: TimelineEntry {
            id: cid(n),
            kind: CheckpointKind::BeforeTurn,
            thread: None,
            turn: None,
        },
        at: at(n),
        tool_paths: None,
    }
}

fn kinds(checkpoint: &Checkpoint) -> Vec<(String, ChangeKind)> {
    checkpoint
        .changes
        .iter()
        .map(|change| (change.path().as_str().to_owned(), change.change().clone()))
        .collect()
}

/// The step adds `sub/new.txt`, in a folder the walk lists after it probes `keep.txt`.
fn added(folder: &Path) {
    put(folder, "sub/new.txt", b"added by the step");
}

/// The case every test but the rename shares: the undo of the added `sub/new.txt`, which comes
/// back cloud-only at `when`.
fn added_comes_back_cloud_only(name: &str, when: resurrect::When) {
    let mut setup = Setup::new(name, added);
    setup.bring_back("sub/new.txt", when, true);
    let undone = setup.undo("sub/new.txt");
    assert_eq!(undone.restored, paths(&["sub/new.txt"]));
    assert_eq!(undone.resurrected, paths(&["sub/new.txt"]));
    assert!(
        setup.exists("sub/new.txt"),
        "the placeholder is in the folder"
    );
    assert_eq!(
        setup.tree(&undone.checkpoint),
        ["keep.txt", "memo.txt"],
        "the undo checkpoint keeps the file it deleted"
    );
    assert_eq!(
        undone.checkpoint.outcome,
        CheckpointOutcome::Created(cid(4))
    );
    // A file that left the tree because it is skipped now is no deletion (`diff`): the
    // checkpoint lists it as skipped, and `resurrected` says why.
    assert_eq!(kinds(&undone.checkpoint), []);
    let skipped: Vec<_> = undone
        .checkpoint
        .skipped
        .iter()
        .map(|file| (file.path.as_str(), file.reason))
        .collect();
    assert_eq!(skipped, [("sub/new.txt", SkippedFileReason::CloudOnly)]);
    assert_eq!(
        fault::failed(&setup.folder.join("sub/new.txt")),
        0,
        "the placeholder is never read"
    );
    // The cache was put back whole: the next checkpoint finds nothing new.
    let next = setup
        .core
        .take(&setup.folder, &setup.settings, &spec(5))
        .unwrap();
    assert_eq!(next.outcome, CheckpointOutcome::Reused(cid(4)));
}

#[test]
fn an_added_file_back_cloud_only_at_once_leaves_the_undo_tree() {
    added_comes_back_cloud_only("undo-back-at-once", resurrect::When::Immediately);
}

#[test]
fn an_added_file_back_cloud_only_during_the_walk_leaves_the_undo_tree() {
    added_comes_back_cloud_only("undo-back-in-walk", resurrect::When::AtNextProbe);
}

#[test]
fn a_renamed_file_back_cloud_only_under_its_new_name_leaves_only_the_old_name() {
    let mut setup = Setup::new("undo-back-renamed", |folder| {
        write::rename(&folder.join("memo.txt"), &folder.join("memo-final.txt")).unwrap();
    });
    setup.bring_back("memo-final.txt", resurrect::When::Immediately, true);
    let undone = setup.undo("memo-final.txt");
    assert_eq!(undone.restored, paths(&["memo-final.txt"]));
    assert_eq!(undone.resurrected, paths(&["memo-final.txt"]));
    assert_eq!(undone.deleted, paths(&["memo-final.txt"]));
    assert!(setup.exists("memo.txt") && setup.exists("memo-final.txt"));
    assert_eq!(setup.tree(&undone.checkpoint), ["keep.txt", "memo.txt"]);
    assert_eq!(
        undone.checkpoint.outcome,
        CheckpointOutcome::Created(cid(4))
    );
}

#[test]
fn an_added_file_back_downloaded_is_recorded_as_the_folder_holds_it() {
    let mut setup = Setup::new("undo-back-downloaded", added);
    setup.bring_back("sub/new.txt", resurrect::When::AtNextProbe, false);
    let undone = setup.undo("sub/new.txt");
    assert_eq!(undone.restored, paths(&["sub/new.txt"]));
    assert_eq!(undone.resurrected, paths(&["sub/new.txt"]));
    // Read back with the bytes it had: the folder is the step's again, so its checkpoint.
    assert_eq!(
        setup.tree(&undone.checkpoint),
        ["keep.txt", "memo.txt", "sub/new.txt"]
    );
    assert_eq!(undone.checkpoint.outcome, CheckpointOutcome::Reused(cid(2)));
}

#[test]
fn an_undo_with_nothing_back_resurrects_nothing() {
    let mut setup = Setup::new("undo-nothing-back", added);
    let undone = setup.undo("sub/new.txt");
    assert_eq!(undone.restored, paths(&["sub/new.txt"]));
    assert_eq!(undone.resurrected, []);
    assert!(!setup.exists("sub/new.txt"));
    assert_eq!(setup.tree(&undone.checkpoint), ["keep.txt", "memo.txt"]);
    assert_eq!(
        undone.checkpoint.outcome,
        CheckpointOutcome::Created(cid(4))
    );
    assert_eq!(
        kinds(&undone.checkpoint),
        [("sub/new.txt".to_owned(), ChangeKind::Deleted)]
    );
    assert_eq!(undone.checkpoint.skipped, []);
}

#[test]
fn back_again_counts_a_placeholder_and_a_file_and_never_a_missing_one() {
    let setup = Setup::new("undo-back-again", added);
    put(&setup.folder, "cloud.txt", b"a placeholder now");
    let cloud = setup.folder.join("cloud.txt");
    setup.marked.lock().unwrap().insert(cloud.clone());
    // A read of the placeholder would fail: `back_again` never reads.
    fault::fail(&cloud, 0, std::io::ErrorKind::Deadlock);
    let asked = paths(&["sub/new.txt", "missing.txt", "cloud.txt", "sub/gone.txt"]);
    assert_eq!(
        super::back_again(&setup.folder, &asked),
        paths(&["cloud.txt", "sub/new.txt"])
    );
    assert_eq!(fault::failed(&cloud), 0);
}

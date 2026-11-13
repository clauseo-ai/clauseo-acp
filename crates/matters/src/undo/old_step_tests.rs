//! Undo of a step whose first checkpoint has no skip record it can read (a Clauseo from before
//! records wrote it, or its record has a line that does not read): what was in the folder at the
//! step's start is unknown, so no file the step added is deleted (each is kept, with an unknown
//! reason), not even one the agent made. Modified files are put back as ever.

use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_protocol::{
    CheckpointId, CheckpointKind, FileChoice, FileDecision, MatterPath, SkippedFile,
    SkippedFileReason, Timestamp,
};

use super::{step, undo};
use crate::checkpoint::{Core, Spec};
use crate::history::{GixStore, HistoryStore};
use crate::skip_record;
use crate::test_dir::TestDir;
use crate::trailers::before_marker;
use crate::types::{Limits, Retention, Settings, TimelineEntry, UndoRequest};
use crate::writeback::Op;
use crate::{PlaceholderProbe, write};

/// Cloud-only for the paths marked.
#[derive(Default)]
struct Probe {
    marked: Arc<Mutex<HashSet<PathBuf>>>,
}

impl PlaceholderProbe for Probe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        self.marked.lock().unwrap().contains(path)
    }
}

fn put(folder: &Path, path: &str, bytes: &[u8]) {
    write::replace(&folder.join(path), &mut |file| {
        file.write_all(bytes).unwrap();
        Ok(())
    })
    .unwrap();
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
            kind: CheckpointKind::AfterTurn,
            thread: None,
            turn: None,
        },
        at: at(n),
        tool_paths: None,
    }
}

fn mpath(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

#[test]
fn an_old_step_keeps_every_added_file_and_says_why_is_unknown() {
    let dir = TestDir::new("undo-old-step");
    let folder = dir.path().join("matter");
    write::create_dir_all(&folder).unwrap();
    let git_dir = dir.path().join("root/m/history.git");
    let probe = Arc::new(Probe::default());
    let settings = Settings {
        matters_root: dir.path().join("root"),
        refused_trees: Vec::new(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::clone(&probe) as Arc<dyn PlaceholderProbe + Send + Sync>,
        trash: Arc::new(crate::FolderTrash::new(dir.path().join("trash"))),
    };
    let mut core = Core::load(GixStore::create(&git_dir, &folder).unwrap()).unwrap();
    put(&folder, "read.txt", b"read v1");
    put(&folder, "notes.txt", b"the lawyer's notes");
    probe
        .marked
        .lock()
        .unwrap()
        .insert(folder.join("notes.txt"));
    before_marker::write(|| core.take(&folder, &settings, &spec(1))).unwrap();

    // The turn, recorded the old way too: the placeholder downloaded unchanged, a file made, one
    // made and gone again, one edited.
    probe.marked.lock().unwrap().clear();
    put(&folder, "new.txt", b"made by the agent");
    put(&folder, "brief.txt", b"made, then removed");
    put(&folder, "read.txt", b"read v2");
    before_marker::write(|| core.take(&folder, &settings, &spec(2))).unwrap();
    write::remove_file(&folder.join("brief.txt")).unwrap();
    drop(core);
    let mut core = Core::load(GixStore::open(&git_dir, &folder).unwrap()).unwrap();

    let steps = step(&core, &folder, &settings, cid(1), cid(2)).unwrap();
    let summary: Vec<(&str, Option<SkippedFileReason>, Vec<&Op>)> = steps
        .iter()
        .map(|step| {
            (
                step.path.as_str(),
                step.unversioned,
                step.ops.iter().collect(),
            )
        })
        .collect();
    let unknown = Some(SkippedFileReason::Unknown);
    assert!(matches!(
        summary.as_slice(),
        [
            ("brief.txt", brief_reason, brief),
            ("new.txt", new_reason, new),
            ("notes.txt", notes_reason, notes),
            ("read.txt", None, read),
        ] if *brief_reason == unknown && brief.is_empty()
            && *new_reason == unknown && new.is_empty()
            && *notes_reason == unknown && notes.is_empty()
            && matches!(read.as_slice(), [Op::Write(..)])
    ));

    // brief.txt is gone already, and kept as the lawyer chose.
    let decisions: Vec<FileDecision> = [
        ("brief.txt", FileChoice::Keep),
        ("new.txt", FileChoice::Undo),
        ("notes.txt", FileChoice::Undo),
        ("read.txt", FileChoice::Undo),
    ]
    .iter()
    .map(|(path, choice)| FileDecision {
        path: mpath(path),
        choice: *choice,
    })
    .collect();
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
    let undone = undo(&mut core, &folder, &settings, &request).unwrap();
    let kept = |path: &str| SkippedFile {
        path: mpath(path),
        reason: SkippedFileReason::Unknown,
    };
    assert_eq!(undone.unversioned, [kept("new.txt"), kept("notes.txt")]);
    assert_eq!(undone.restored, [mpath("read.txt")]);
    assert_eq!(
        undone.kept,
        [mpath("brief.txt"), mpath("new.txt"), mpath("notes.txt")]
    );
    assert_eq!(undone.deleted, [] as [MatterPath; 0]);
    assert_eq!(std::fs::read(folder.join("read.txt")).unwrap(), b"read v1");
    assert_eq!(
        std::fs::read(folder.join("notes.txt")).unwrap(),
        b"the lawyer's notes"
    );
    assert_eq!(
        std::fs::read(folder.join("new.txt")).unwrap(),
        b"made by the agent"
    );
}

#[test]
fn a_step_whose_start_record_does_not_read_keeps_every_added_file() {
    let dir = TestDir::new("undo-corrupt-record");
    let folder = dir.path().join("matter");
    write::create_dir_all(&folder).unwrap();
    let git_dir = dir.path().join("root/m/history.git");
    let probe = Arc::new(Probe::default());
    let settings = Settings {
        matters_root: dir.path().join("root"),
        refused_trees: Vec::new(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::clone(&probe) as Arc<dyn PlaceholderProbe + Send + Sync>,
        trash: Arc::new(crate::FolderTrash::new(dir.path().join("trash"))),
    };
    let mut core = Core::load(GixStore::create(&git_dir, &folder).unwrap()).unwrap();
    put(&folder, "read.txt", b"read v1");
    put(&folder, "notes.txt", b"the lawyer's notes");
    probe
        .marked
        .lock()
        .unwrap()
        .insert(folder.join("notes.txt"));
    // The record of the step's start has a line no reader can read.
    skip_record::corrupt::write(|| core.take(&folder, &settings, &spec(1))).unwrap();
    assert_eq!(core.skip_record(cid(1)), None);

    put(&folder, "new.txt", b"made by the agent");
    put(&folder, "read.txt", b"read v2");
    core.take(&folder, &settings, &spec(2)).unwrap();
    let decisions: Vec<FileDecision> = ["new.txt", "read.txt"]
        .iter()
        .map(|path| FileDecision {
            path: mpath(path),
            choice: FileChoice::Undo,
        })
        .collect();
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
    let undone = undo(&mut core, &folder, &settings, &request).unwrap();
    assert_eq!(
        undone.unversioned,
        [SkippedFile {
            path: mpath("new.txt"),
            reason: SkippedFileReason::Unknown,
        }]
    );
    assert_eq!(undone.restored, [mpath("read.txt")]);
    assert_eq!(undone.deleted, [] as [MatterPath; 0]);
    assert_eq!(
        std::fs::read(folder.join("new.txt")).unwrap(),
        b"made by the agent"
    );
    assert_eq!(std::fs::read(folder.join("read.txt")).unwrap(), b"read v1");
}

//! A matter per test: fixed ids and times, a folder and a matters root inside one `TestDir`,
//! and the expected `FileChange` values built from file contents.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clauseo_matters::{
    Checkpoint, DEFAULT_MARKER, Error, Imported, Limits, Matter, NewMatter, Restore, RestoreFiles,
    Settings, TurnMeta, UndoRequest, Undone,
};
use clauseo_protocol::{
    ChangeKind, CheckpointId, ContentId, Count, FileChange, FileChoice, FileDecision,
    ImportSummary, MatterId, MatterPath, ThreadId, Timestamp, TurnId,
};

use crate::support::{self, MarkedProbe, TestDir};

/// Checkpoint `n`: `01900000-0000-7000-8000-<n>`. The root checkpoint of every matter is 1.
pub(crate) fn cid(n: u64) -> CheckpointId {
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

/// Matter `n`.
pub(crate) fn matter_id(n: u64) -> MatterId {
    format!("01910000-0000-7000-8000-{n:012x}").parse().unwrap()
}

/// The one thread the tests' turns run in.
pub(crate) fn thread() -> ThreadId {
    "01920000-0000-7000-8000-000000000001".parse().unwrap()
}

/// Turn `n`.
pub(crate) fn turn(n: u64) -> TurnId {
    format!("01930000-0000-7000-8000-{n:012x}").parse().unwrap()
}

/// A fixed time `n` seconds after 2023-11-14.
pub(crate) fn at(n: u64) -> Timestamp {
    Timestamp::new(1_700_000_000_000 + i64::try_from(n).unwrap() * 1000).unwrap()
}

/// The checkpoint `n` of turn `n`, in [`thread`].
pub(crate) fn meta(n: u64) -> TurnMeta {
    TurnMeta {
        checkpoint: cid(n),
        thread: thread(),
        turn: turn(n),
        at: at(n),
    }
}

pub(crate) fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

/// The set of tool-call paths.
pub(crate) fn tools(paths: &[&str]) -> BTreeSet<MatterPath> {
    paths.iter().map(|text| path(text)).collect()
}

/// The git blob id of `bytes`, as a content id.
pub(crate) fn blob(bytes: &[u8]) -> ContentId {
    let id = gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, bytes).unwrap();
    ContentId::new(&id.to_hex().to_string()).unwrap()
}

fn size(bytes: &[u8]) -> Count {
    Count::clamped(u64::try_from(bytes.len()).unwrap())
}

pub(crate) fn added(at_path: &str, bytes: &[u8]) -> FileChange {
    let kind = ChangeKind::Added;
    FileChange::new(path(at_path), kind, None, Some(blob(bytes)), size(bytes)).unwrap()
}

pub(crate) fn modified(at_path: &str, old: &[u8], new: &[u8]) -> FileChange {
    let (before, after) = (Some(blob(old)), Some(blob(new)));
    FileChange::new(
        path(at_path),
        ChangeKind::Modified,
        before,
        after,
        size(new),
    )
    .unwrap()
}

pub(crate) fn deleted(at_path: &str, old: &[u8]) -> FileChange {
    let kind = ChangeKind::Deleted;
    FileChange::new(path(at_path), kind, Some(blob(old)), None, size(old)).unwrap()
}

pub(crate) fn renamed(from: &str, to: &str, bytes: &[u8]) -> FileChange {
    let kind = ChangeKind::Renamed { from: path(from) };
    let id = Some(blob(bytes));
    FileChange::new(path(to), kind, id.clone(), id, size(bytes)).unwrap()
}

/// Every path decided `Undo`.
pub(crate) fn undo_all(paths: &[&str]) -> Vec<FileDecision> {
    paths
        .iter()
        .map(|text| FileDecision {
            path: path(text),
            choice: FileChoice::Undo,
        })
        .collect()
}

/// An undo of `before..after` with `decisions`, outside-edits id `outside` and undo id `undo`.
pub(crate) fn undo_request(
    before: u64,
    after: u64,
    decisions: &[FileDecision],
    outside: u64,
    undo: u64,
) -> UndoRequest<'_> {
    UndoRequest {
        before: cid(before),
        after: cid(after),
        decisions,
        thread: Some(thread()),
        turn: Some(turn(after)),
        outside_edits: cid(outside),
        checkpoint: cid(undo),
        at: at(undo),
    }
}

/// Undoes `before..after`, every path in `paths` decided `Undo`.
pub(crate) fn undo(
    matter: &mut Matter,
    (before, after): (u64, u64),
    paths: &[&str],
    (outside, checkpoint): (u64, u64),
) -> Result<Undone, Error> {
    let decisions = undo_all(paths);
    matter.undo(&undo_request(
        before, after, &decisions, outside, checkpoint,
    ))
}

/// A restore of checkpoint `from`, outside-edits id `outside` and restore id `checkpoint`.
pub(crate) fn restore_request(
    from: u64,
    files: RestoreFiles<'_>,
    outside: u64,
    checkpoint: u64,
) -> Restore<'_> {
    Restore {
        from: cid(from),
        files,
        outside_edits: cid(outside),
        checkpoint: cid(checkpoint),
        at: at(checkpoint),
    }
}

/// The bytes of `rel` at checkpoint `n`.
pub(crate) fn read_at(matter: &Matter, n: u64, rel: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    matter.read_file(cid(n), &path(rel), &mut bytes).unwrap();
    bytes
}

/// A test's matter: `<dir>/matter` is the folder and `<dir>/root` the matters root.
pub(crate) struct Fixture {
    pub(crate) dir: TestDir,
    pub(crate) probe: Arc<MarkedProbe>,
    pub(crate) folder: PathBuf,
    pub(crate) limits: Limits,
}

impl Fixture {
    pub(crate) fn new(name: &str) -> Self {
        let dir = TestDir::new(name);
        let folder = dir.path().join("matter");
        Self {
            dir,
            probe: Arc::default(),
            folder,
            limits: Limits::DEFAULT,
        }
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.dir.path().join("root")
    }

    pub(crate) fn settings(&self) -> Settings {
        let mut settings = support::settings(self.root(), Arc::clone(&self.probe));
        settings.limits = self.limits;
        settings
    }

    /// Matter 1 in the fixture's folder, root checkpoint 1 at time 0.
    pub(crate) fn new_matter(&self) -> NewMatter {
        NewMatter {
            id: matter_id(1),
            folder: self.folder.clone(),
            created_at: at(0),
            marker: DEFAULT_MARKER,
            checkpoint: cid(1),
        }
    }

    /// `<root>/<matter 1>`.
    pub(crate) fn home(&self) -> PathBuf {
        self.root().join(matter_id(1).to_string())
    }

    pub(crate) fn git_dir(&self) -> PathBuf {
        self.home().join("history.git")
    }

    /// `rel` inside the folder.
    pub(crate) fn file(&self, rel: &str) -> PathBuf {
        self.folder.join(rel)
    }

    pub(crate) fn write(&self, rel: &str, contents: impl AsRef<[u8]>) {
        support::write(self.file(rel), contents);
    }

    pub(crate) fn read(&self, rel: &str) -> Vec<u8> {
        std::fs::read(self.file(rel)).unwrap()
    }

    pub(crate) fn exists(&self, rel: &str) -> bool {
        self.file(rel).symlink_metadata().is_ok()
    }

    /// Imports the folder as matter 1.
    pub(crate) fn import(&self) -> Matter {
        self.import_with_summary().0
    }

    pub(crate) fn import_with_summary(&self) -> (Matter, ImportSummary) {
        let cancel = AtomicBool::new(false);
        let imported = Matter::import(
            self.settings(),
            self.new_matter(),
            &[],
            &mut |_| {},
            &cancel,
        )
        .unwrap();
        match imported {
            Imported::Done {
                matter, summary, ..
            } => (*matter, summary),
            Imported::Cancelled => panic!("cancelled"),
        }
    }

    /// Opens matter 1 in `folder`.
    pub(crate) fn open(&self, folder: &Path) -> Result<Matter, Error> {
        Matter::open(self.settings(), matter_id(1), folder)
    }
}

/// Asserts no undo temp file (`.clauseo-<n>.tmp`) is left anywhere under `dir`.
pub(crate) fn assert_no_temp_files(dir: &Path) {
    let mut stack = vec![dir.to_owned()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            assert!(
                !(name.starts_with(".clauseo-")
                    && Path::new(&name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))),
                "{} left behind",
                entry.path().display()
            );
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            }
        }
    }
}

/// Asserts `items` is empty, showing them if not.
pub(crate) fn assert_none<T: PartialEq + std::fmt::Debug>(items: &[T]) {
    assert_eq!(items, &[] as &[T]);
}

/// A checkpoint's changes, asserting it was created with id `n`.
pub(crate) fn created(checkpoint: Checkpoint, n: u64) -> Vec<FileChange> {
    assert_eq!(
        checkpoint.outcome,
        clauseo_matters::CheckpointOutcome::Created(cid(n)),
        "{checkpoint:?}"
    );
    checkpoint.changes
}

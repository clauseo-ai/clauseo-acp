//! Tests of where undo's and restore's deletes go, with a trash the test sets: moved to the
//! trash; deleted for good with one warning when the trash fails or the volume has none, the
//! history holding the file; nothing deleted when the save before the writeback fails.

use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_protocol::{
    CheckpointId, CheckpointKind, FileChoice, FileDecision, MatterPath, Timestamp,
};
use tracing::field::{Field, Visit};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, span};

use super::{FolderTrash, Removals, Trash, TrashError};
use crate::checkpoint::{Core, Spec};
use crate::history::{GixStore, HistoryStore};
use crate::no_download::fault;
use crate::test_dir::TestDir;
use crate::types::{Limits, Restore, RestoreFiles, Retention, Settings, TimelineEntry};
use crate::types::{UndoRequest, Undone};
use crate::{Error, OsProbe, write};

/// A trash that answers as told, recording each path it was asked to take.
struct Told {
    answer: fn(&Path) -> Result<Option<PathBuf>, TrashError>,
    asked: Mutex<Vec<PathBuf>>,
}

impl Trash for Told {
    fn trash(&self, path: &Path) -> Result<Option<PathBuf>, TrashError> {
        self.asked.lock().unwrap().push(path.to_owned());
        (self.answer)(path)
    }
}

fn told(answer: fn(&Path) -> Result<Option<PathBuf>, TrashError>) -> Arc<Told> {
    Arc::new(Told {
        answer,
        asked: Mutex::new(Vec::new()),
    })
}

/// A matter with `keep.txt` at checkpoint 1, and checkpoint 2 after `change`.
struct Setup {
    dir: TestDir,
    folder: PathBuf,
    settings: Settings,
    core: Core,
}

impl Setup {
    /// `trash`, or by default a [`FolderTrash`] at [`Setup::bin`].
    fn new(name: &str, trash: Option<Arc<dyn Trash>>, change: impl FnOnce(&Path)) -> Self {
        fault::clear();
        let dir = TestDir::new(name);
        let trash = trash.unwrap_or_else(|| Arc::new(FolderTrash::new(dir.path().join("bin"))));
        let folder = dir.path().join("matter");
        write::create_dir_all(&folder).unwrap();
        let root = dir.path().join("root");
        let settings = Settings {
            matters_root: root.clone(),
            refused_trees: Vec::new(),
            limits: Limits::DEFAULT,
            retention: Retention::KeepAll,
            probe: Arc::new(OsProbe),
            trash,
        };
        let git_dir = root.join("m").join("history.git");
        let mut core = Core::load(GixStore::create(&git_dir, &folder).unwrap()).unwrap();
        put(&folder, "keep.txt", b"kept as it is");
        core.take(&folder, &settings, &spec(1)).unwrap();
        change(&folder);
        core.take(&folder, &settings, &spec(2)).unwrap();
        Self {
            dir,
            folder,
            settings,
            core,
        }
    }

    /// Undoes the step 1..2 as checkpoints 3 and 4: each of `undo` decided `Undo`, each of
    /// `keep` `Keep`.
    fn undo(&mut self, undo: &[&str], keep: &[&str]) -> Result<Undone, Error> {
        let decide = |paths: &[&str], choice: FileChoice| {
            paths
                .iter()
                .map(|path| FileDecision {
                    path: MatterPath::new(path).unwrap(),
                    choice,
                })
                .collect::<Vec<_>>()
        };
        let mut decisions = decide(undo, FileChoice::Undo);
        decisions.extend(decide(keep, FileChoice::Keep));
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
        crate::undo::undo(&mut self.core, &self.folder, &self.settings, &request)
    }

    fn file(&self, path: &str) -> PathBuf {
        self.folder.join(path)
    }

    /// The default trash's folder.
    fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    /// The bytes of each file in the default trash.
    fn in_bin(&self) -> Vec<Vec<u8>> {
        std::fs::read_dir(self.bin())
            .unwrap()
            .map(|entry| std::fs::read(entry.unwrap().path()).unwrap())
            .collect()
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

/// The step adds `new.txt` and `sub/also.txt`.
fn two_added(folder: &Path) {
    put(folder, "new.txt", b"added by the step");
    put(folder, "sub/also.txt", b"also added");
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

/// The fields of each warning `run` logs about deletes without the trash, as text.
fn fallback_warnings<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Warnings {
        lines: Arc::clone(&lines),
    };
    let out = tracing::subscriber::with_default(subscriber, run);
    let lines = lines.lock().unwrap().clone();
    let fallbacks = lines
        .into_iter()
        .filter(|line| line.contains("without the trash"))
        .collect();
    (out, fallbacks)
}

/// Records each warning's fields as `name=value` text.
struct Warnings {
    lines: Arc<Mutex<Vec<String>>>,
}

struct Fields<'a>(&'a mut String);

impl Visit for Fields<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        write!(self.0, "{}={value:?} ", field.name()).unwrap();
    }
}

impl tracing::Subscriber for Warnings {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() == Level::WARN
    }
    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }
    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut line = String::new();
        event.record(&mut Fields(&mut line));
        self.lines.lock().unwrap().push(line);
    }
    fn enter(&self, _: &span::Id) {}
    fn exit(&self, _: &span::Id) {}
}

#[test]
fn an_undone_added_file_goes_to_the_trash_with_its_bytes() {
    let mut setup = Setup::new("trash-ok", None, two_added);

    let (undone, warnings) = fallback_warnings(|| setup.undo(&["new.txt"], &["sub/also.txt"]));

    assert!(!setup.file("new.txt").exists(), "gone from the matter");
    assert_eq!(setup.in_bin(), [b"added by the step".to_vec()]);
    assert_eq!(
        undone.unwrap().removals,
        Removals {
            trashed: 1,
            deleted: 0
        }
    );
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_failed_trash_deletes_the_saved_files_with_one_warning_and_no_path() {
    let trash = told(|_| Err(TrashError::Failed(io::ErrorKind::PermissionDenied.into())));
    let mut setup = Setup::new("trash-fails", Some(Arc::<Told>::clone(&trash)), two_added);

    let (undone, warnings) = fallback_warnings(|| setup.undo(&["new.txt", "sub/also.txt"], &[]));
    let undone = undone.unwrap();

    assert!(!setup.file("new.txt").exists());
    assert!(!setup.file("sub/also.txt").exists());
    assert_eq!(
        trash.asked.lock().unwrap().len(),
        2,
        "each tried the trash first"
    );
    assert_eq!(
        undone.removals,
        Removals {
            trashed: 0,
            deleted: 2
        }
    );
    assert_eq!(warnings.len(), 1, "one warning for the job: {warnings:?}");
    assert!(warnings[0].contains("files=2"), "{warnings:?}");
    assert!(warnings[0].contains("permission denied"), "{warnings:?}");
    let folder = setup.folder.to_string_lossy().into_owned();
    assert!(!warnings[0].contains(&folder), "no path: {warnings:?}");
    assert!(!warnings[0].contains("new.txt"), "no path: {warnings:?}");
    // The history holds both: the after-step checkpoint has them byte for byte.
    let files = setup.core.files_of(cid(2)).unwrap();
    assert!(files.iter().any(|file| file.path.as_str() == "new.txt"));
    assert!(
        files
            .iter()
            .any(|file| file.path.as_str() == "sub/also.txt")
    );
}

#[test]
fn a_volume_without_a_trash_falls_back_to_the_delete() {
    let trash = told(|_| Err(TrashError::NoTrash));
    let mut setup = Setup::new("trash-none", Some(Arc::<Told>::clone(&trash)), two_added);

    let (undone, warnings) = fallback_warnings(|| setup.undo(&["new.txt"], &["sub/also.txt"]));

    assert!(!setup.file("new.txt").exists());
    assert!(setup.file("sub/also.txt").exists(), "kept: not chosen");
    assert_eq!(trash.asked.lock().unwrap().len(), 1);
    assert_eq!(
        undone.unwrap().removals,
        Removals {
            trashed: 0,
            deleted: 1
        }
    );
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("no trash on the volume"),
        "{warnings:?}"
    );
}

#[test]
fn an_os_without_a_trash_deletes_with_no_warning() {
    let trash = told(|_| Err(TrashError::Unsupported));
    let mut setup = Setup::new("trash-unsupported", Some(trash), two_added);

    let (undone, warnings) = fallback_warnings(|| setup.undo(&["new.txt"], &["sub/also.txt"]));

    assert!(!setup.file("new.txt").exists());
    assert_eq!(undone.unwrap().removals.deleted, 1);
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// The order the fallback relies on: undo saves the folder (its `outside-edits` checkpoint)
/// before any delete, and a failed save fails the undo with nothing deleted or trashed.
#[test]
fn a_failed_save_before_the_undo_deletes_nothing() {
    let trash = told(|_| Err(TrashError::NoTrash));
    let mut setup = Setup::new(
        "trash-save-fails",
        Some(Arc::<Told>::clone(&trash)),
        two_added,
    );
    // The lawyer edits the added file after the step: the save must read it, and cannot.
    write::append(&setup.file("new.txt"), b" and the lawyer's edit").unwrap();
    fault::fail(&setup.file("new.txt"), 0, io::ErrorKind::PermissionDenied);

    let err = setup.undo(&["new.txt", "sub/also.txt"], &[]).unwrap_err();
    fault::clear();

    assert!(
        matches!(&err, Error::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied),
        "{err:?}"
    );
    assert_eq!(
        std::fs::read(setup.file("new.txt")).unwrap(),
        b"added by the step and the lawyer's edit"
    );
    assert!(setup.file("sub/also.txt").exists());
    assert!(
        trash.asked.lock().unwrap().is_empty(),
        "nothing went to the trash"
    );
}

/// The save comes first: the edited file's bytes are in the `outside-edits` checkpoint when
/// the trash is asked for it.
#[test]
fn the_save_is_committed_before_the_trash_is_asked() {
    let trash = told(|_| Err(TrashError::NoTrash));
    let mut setup = Setup::new(
        "trash-save-first",
        Some(Arc::<Told>::clone(&trash)),
        two_added,
    );
    write::append(&setup.file("new.txt"), b" edited").unwrap();

    let undone = setup.undo(&["new.txt"], &["sub/also.txt"]).unwrap();

    let outside = undone.outside_edits.expect("the edit saved");
    assert_eq!(trash.asked.lock().unwrap().len(), 1);
    let files = setup.core.files_of(cid(3)).unwrap();
    let saved = files
        .iter()
        .find(|file| file.path.as_str() == "new.txt")
        .unwrap();
    let mut bytes = Vec::new();
    setup
        .core
        .history
        .read_blob(saved.blob, "new.txt", &mut bytes)
        .unwrap();
    assert_eq!(bytes, b"added by the step edited");
    assert!(matches!(
        outside.outcome,
        crate::CheckpointOutcome::Created(id) if id == cid(3)
    ));
}

/// A restore that undoes an earlier restore deletes through the same trash.
#[test]
fn a_restore_undoing_a_restore_trashes_the_file_it_added() {
    let mut setup = Setup::new("trash-restore", None, |folder| {
        put(folder, "gone.txt", b"deleted later");
    });
    write::remove_file(&setup.file("gone.txt")).unwrap();
    setup
        .core
        .take(&setup.folder, &setup.settings, &spec(3))
        .unwrap();
    let first = Restore {
        from: cid(2),
        files: RestoreFiles::All,
        outside_edits: cid(4),
        checkpoint: cid(5),
        at: at(5),
        undoes: None,
    };
    crate::restore::restore(&mut setup.core, &setup.folder, &setup.settings, &first).unwrap();
    assert!(
        setup.file("gone.txt").exists(),
        "the restore brought it back"
    );
    let back = Restore {
        from: cid(3),
        files: RestoreFiles::All,
        outside_edits: cid(6),
        checkpoint: cid(7),
        at: at(7),
        undoes: Some(cid(5)),
    };

    let restored =
        crate::restore::restore(&mut setup.core, &setup.folder, &setup.settings, &back).unwrap();

    assert_eq!(restored.removed, [MatterPath::new("gone.txt").unwrap()]);
    assert!(!setup.file("gone.txt").exists());
    assert_eq!(setup.in_bin(), [b"deleted later".to_vec()]);
}

#[test]
fn the_folder_trash_names_each_file_apart() {
    let dir = TestDir::new("trash-folder-names");
    let trash = FolderTrash::new(dir.path().join("bin"));
    put(dir.path(), "a/memo.txt", b"one");
    put(dir.path(), "b/memo.txt", b"two");

    let first = trash
        .trash(&dir.path().join("a/memo.txt"))
        .unwrap()
        .unwrap();
    let second = trash
        .trash(&dir.path().join("b/memo.txt"))
        .unwrap()
        .unwrap();
    let missing = trash.trash(&dir.path().join("a/memo.txt")).unwrap_err();

    assert_ne!(first, second);
    assert_eq!(std::fs::read(first).unwrap(), b"one");
    assert_eq!(std::fs::read(second).unwrap(), b"two");
    assert!(
        matches!(&missing, TrashError::Failed(err) if err.kind() == io::ErrorKind::NotFound),
        "{missing:?}"
    );
}

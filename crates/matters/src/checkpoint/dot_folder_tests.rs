//! Tests of a history written before every dot-folder was left out (`.claude`, `.codex`):
//! its files under one are shown nowhere, never reported deleted, and never undone.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{
    ChangeKind, CheckpointId, CheckpointKind, FileChoice, FileDecision, MatterPath, Timestamp,
};

use super::{Core, Spec};
use crate::compare::{compare, files_at};
use crate::history::{GixStore, HistoryStore};
use crate::no_download::NoDownload;
use crate::skip_record::{SkipEntry, SkipRecord};
use crate::stat::{CacheEntry, EntryState, Stat};
use crate::test_dir::TestDir;
use crate::types::{
    Checkpoint, CompareSide, Limits, Retention, Settings, TimelineEntry, UndoRequest,
};
use crate::{Error, OsProbe, undo, write};

const SETTINGS_V1: &[u8] = b"{\"permissions\":{\"allow\":[]}}";
const SETTINGS_V2: &[u8] = b"{\"permissions\":{\"allow\":[\"Bash\"]}}";

struct Setup {
    _dir: TestDir,
    folder: PathBuf,
    settings: Settings,
    core: Core,
}

impl Setup {
    fn new(name: &str) -> Self {
        let dir = TestDir::new(name);
        let folder = dir.path().join("matter");
        write::create_dir_all(&folder).unwrap();
        let root = dir.path().join("root");
        let settings = Settings {
            matters_root: root.clone(),
            refused_trees: Vec::new(),
            limits: Limits::DEFAULT,
            retention: Retention::KeepAll,
            probe: Arc::new(OsProbe),
        };
        let git_dir = root.join("m").join("history.git");
        let core = Core::load(GixStore::create(&git_dir, &folder).unwrap()).unwrap();
        Self {
            _dir: dir,
            folder,
            settings,
            core,
        }
    }

    fn put(&self, path: &str, bytes: &[u8]) {
        let abs = self.folder.join(path);
        write::create_dir_all(abs.parent().unwrap()).unwrap();
        write::replace(&abs, &mut |file| {
            file.write_all(bytes).unwrap();
            Ok(())
        })
        .unwrap();
    }

    fn read(&self, path: &str) -> Vec<u8> {
        std::fs::read(self.folder.join(path)).unwrap()
    }

    /// Checkpoint `n` as a build before the rule took it: the walk, and `old` (files under
    /// dot-folders) read and versioned as well.
    fn take_before_rule(&mut self, n: u64, old: &[&str]) -> Checkpoint {
        let mut walked = self
            .core
            .scan_folder(&self.folder, &self.settings, None, &mut |_| {})
            .unwrap()
            .unwrap();
        let guard = NoDownload::begin(self.settings.probe.as_ref());
        for path in old {
            let abs = self.folder.join(path);
            let metadata = std::fs::symlink_metadata(&abs).unwrap();
            let (blob, _) = self
                .core
                .history
                .store_file(&abs, metadata.len(), u64::MAX, &guard)
                .unwrap();
            walked.entries.push(CacheEntry {
                path: MatterPath::new(path).unwrap(),
                stat: Stat::from_metadata(&metadata),
                state: EntryState::Read(blob),
            });
        }
        walked
            .entries
            .sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
        self.core.commit(walked, &spec(n)).unwrap()
    }

    fn take(&mut self, n: u64) -> Checkpoint {
        self.core
            .take(&self.folder, &self.settings, &spec(n))
            .unwrap()
    }
}

fn cid(n: u64) -> CheckpointId {
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

fn spec(n: u64) -> Spec<'static> {
    Spec {
        entry: TimelineEntry {
            id: cid(n),
            kind: CheckpointKind::BeforeTurn,
            thread: None,
            turn: None,
        },
        at: Timestamp::new(1_700_000_000_000 + i64::try_from(n).unwrap() * 1000).unwrap(),
        tool_paths: None,
    }
}

/// The undo of the step from checkpoint 1 to 2, as checkpoints 3 (outside edits) and 4.
fn request(decisions: &[FileDecision]) -> UndoRequest<'_> {
    UndoRequest {
        before: cid(1),
        after: cid(2),
        decisions,
        thread: None,
        turn: None,
        outside_edits: cid(3),
        checkpoint: cid(4),
        at: Timestamp::new(1_700_000_004_000).unwrap(),
    }
}

fn changed(changes: &[clauseo_protocol::FileChange]) -> Vec<(String, ChangeKind)> {
    changes
        .iter()
        .map(|change| (change.path().as_str().to_owned(), change.change().clone()))
        .collect()
}

#[test]
fn a_dot_folder_file_an_old_checkpoint_versioned_is_never_reported_deleted() {
    let mut setup = Setup::new("dot-folder-no-deletion");
    setup.put("memo.txt", b"the memo");
    setup.put(".claude/settings.local.json", SETTINGS_V1);
    setup.take_before_rule(1, &[".claude/settings.local.json"]);
    let tree = setup
        .core
        .history
        .read_tree(setup.core.timeline.tree(cid(1)).unwrap());
    assert_eq!(tree.unwrap().len(), 2, "the old checkpoint versioned it");
    // Nothing else changed: no deletion, and no new checkpoint for the rule alone.
    let same = setup.take(2);
    assert_eq!(changed(&same.changes), Vec::new());
    // A real change is the only one reported, and the next version leaves the file out.
    setup.put("memo.txt", b"the memo, edited");
    let edited = setup.take(3);
    assert_eq!(
        changed(&edited.changes),
        [("memo.txt".to_owned(), ChangeKind::Modified)]
    );
    let after = setup.core.timeline.entries.last().unwrap().id;
    assert_eq!(
        changed(&setup.core.changes_between(cid(1), after).unwrap()),
        [("memo.txt".to_owned(), ChangeKind::Modified)]
    );
    let files = files_at(&setup.core, cid(1)).unwrap();
    let names: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(names, ["memo.txt"]);
    let now = compare(
        &setup.core,
        &setup.folder,
        &setup.settings,
        CompareSide::Checkpoint(cid(1)),
        CompareSide::Folder,
    );
    assert_eq!(
        changed(&now.unwrap()),
        [("memo.txt".to_owned(), ChangeKind::Modified)]
    );
    assert_eq!(setup.read(".claude/settings.local.json"), SETTINGS_V1);
}

#[test]
fn undo_of_an_old_step_that_edited_a_dot_folder_leaves_it_alone() {
    let mut setup = Setup::new("dot-folder-undo");
    setup.put("memo.txt", b"the memo");
    setup.put(".claude/settings.local.json", SETTINGS_V1);
    setup.take_before_rule(1, &[".claude/settings.local.json"]);
    // The old step edited the settings, added a file under `.codex` and edited the memo.
    setup.put(".claude/settings.local.json", SETTINGS_V2);
    setup.put("a/.codex/notes.md", b"agent notes");
    setup.put("memo.txt", b"the memo, edited");
    // That build reported all three; read now, the step changed only the memo.
    setup.take_before_rule(2, &["a/.codex/notes.md", ".claude/settings.local.json"]);
    let tree = setup
        .core
        .history
        .read_tree(setup.core.timeline.tree(cid(2)).unwrap());
    assert_eq!(tree.unwrap().len(), 3, "the old checkpoint versioned them");
    assert_eq!(
        changed(&setup.core.changes_between(cid(1), cid(2)).unwrap()),
        [("memo.txt".to_owned(), ChangeKind::Modified)]
    );
    // A decision on a dot-folder file names no file of the step.
    let decide = |path: &str| FileDecision {
        path: MatterPath::new(path).unwrap(),
        choice: FileChoice::Undo,
    };
    let refused_decisions = [decide(".claude/settings.local.json")];
    let decisions = [decide("memo.txt")];
    let refused = undo::undo(
        &mut setup.core,
        &setup.folder,
        &setup.settings,
        &request(&refused_decisions),
    );
    assert!(
        matches!(refused, Err(Error::UnexpectedDecision { .. })),
        "{refused:?}"
    );
    let undone = undo::undo(
        &mut setup.core,
        &setup.folder,
        &setup.settings,
        &request(&decisions),
    )
    .unwrap();
    let restored: Vec<&str> = undone.restored.iter().map(MatterPath::as_str).collect();
    assert_eq!(restored, ["memo.txt"]);
    assert_eq!(setup.read("memo.txt"), b"the memo");
    assert_eq!(setup.read(".claude/settings.local.json"), SETTINGS_V2);
    assert_eq!(setup.read("a/.codex/notes.md"), b"agent notes");
    assert_eq!(
        changed(&undone.checkpoint.changes),
        [("memo.txt".to_owned(), ChangeKind::Modified)]
    );
    assert!(setup.folder.join(".claude").is_dir());
}

#[test]
fn undo_keeps_a_paper_an_old_step_moved_out_of_a_dot_folder() {
    let mut setup = Setup::new("dot-folder-moved-out");
    setup.put("memo.txt", b"the memo");
    setup.put(".claude/x.md", b"the agent's notes");
    setup.take_before_rule(1, &[".claude/x.md"]);
    // The old step moved it out to `x.md`: a rename then, an added file now.
    let gone = setup.folder.join(".claude").join("x.md");
    write::remove_file(&gone).unwrap();
    setup.put("x.md", b"the agent's notes");
    setup.take_before_rule(2, &[]);
    assert_eq!(
        changed(&setup.core.changes_between(cid(1), cid(2)).unwrap()),
        [("x.md".to_owned(), ChangeKind::Added)]
    );
    let decisions = [FileDecision {
        path: MatterPath::new("x.md").unwrap(),
        choice: FileChoice::Undo,
    }];
    let undone = undo::undo(
        &mut setup.core,
        &setup.folder,
        &setup.settings,
        &request(&decisions),
    )
    .unwrap();
    assert_eq!(setup.read("x.md"), b"the agent's notes");
    let kept: Vec<&str> = undone.unversioned.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(kept, ["x.md"]);
}

#[test]
fn a_skip_record_never_lists_a_dot_folder_path() {
    let old = "clauseo-skips 1\n\
        skipped cloud-only 18 1700000000.000000000 .claude/big.bin\n\
        stale 20 1700000000.000000000 a/.codex/s.md\n\
        unread .agents/skills\n\
        skipped cloud-only 18 1700000000.000000000 notes.txt\n";
    let record = SkipRecord::decode(old).unwrap();
    let skipped: Vec<&str> = record.skipped.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(skipped, ["notes.txt"]);
    assert_eq!(record.stale, Vec::<SkipEntry>::new());
    assert_eq!(record.unread, Vec::<String>::new());
    let cache = [CacheEntry {
        path: MatterPath::new(".claude/big.bin").unwrap(),
        stat: Stat::default(),
        state: EntryState::Skipped,
    }];
    assert_eq!(SkipRecord::of_cache(&cache, 10), SkipRecord::default());
}

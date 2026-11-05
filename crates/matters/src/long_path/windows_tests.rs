//! Windows: a matter whose history paths run past 260 UTF-16 units, under a matters root with a
//! Devanagari folder. Each step writes loose objects through gix-odb, whose rename does not get
//! std's verbatim prefix; before [`super::long_path`] every checkpoint here failed with
//! `matter.checkpoint_failed` (os error 3), whatever `LongPathsEnabled` said.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use clauseo_protocol::{CheckpointId, FileChoice, FileDecision, MatterId, MatterPath, Timestamp};

use crate::test_dir::TestDir;
use crate::{
    CheckpointOutcome, DEFAULT_MARKER, Imported, Limits, Matter, NewMatter, OsProbe, Restore,
    RestoreFiles, Retention, Settings, TurnMeta, UndoRequest, write,
};

/// The UTF-16 length of `path`, as the 260 limit counts.
fn units(path: &Path) -> usize {
    path.as_os_str().encode_wide().count()
}

/// `base` with folders added (a Devanagari one first) until it is at least `at_least` units.
fn deep(base: &Path, at_least: usize) -> PathBuf {
    let mut path = base.join("मामले-अनुबंध-दस्तावेज़");
    let mut n = 0;
    while units(&path) < at_least {
        n += 1;
        path.push(format!(
            "{n:02}-{}",
            "folder-of-a-deeply-nested-client".repeat(2)
        ));
    }
    path
}

fn cid(n: u64) -> CheckpointId {
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

fn at(n: u64) -> Timestamp {
    Timestamp::new(1_700_000_000_000 + i64::try_from(n).unwrap() * 1000).unwrap()
}

fn meta(n: u64) -> TurnMeta {
    TurnMeta {
        checkpoint: cid(n),
        thread: "01920000-0000-7000-8000-000000000001".parse().unwrap(),
        turn: format!("01930000-0000-7000-8000-{n:012x}").parse().unwrap(),
        at: at(n),
    }
}

fn replace(path: &Path, text: &str) {
    write::replace(path, &mut |file| {
        file.write_all(text.as_bytes()).unwrap();
        Ok(())
    })
    .unwrap();
}

/// Create, two checkpoints, the timeline, an undo and a restore, all with history paths over
/// 300 UTF-16 units and the matter folder over 260; the first object's path is measured, so the
/// test cannot pass with short paths.
#[test]
fn a_matter_with_history_paths_past_260_units_checkpoints_undoes_and_restores() {
    let dir = TestDir::new("long-path-matter");
    let base = deep(dir.path(), 270);
    let matters_root = base.join("root");
    let folder = base.join("मामला");
    write::create_dir_all(&folder).unwrap();
    let lease = folder.join("lease.txt");
    replace(&lease, "first draft\n");
    assert!(units(&folder) > 260, "{}", units(&folder));

    let id: MatterId = "01910000-0000-7000-8000-000000000001".parse().unwrap();
    let settings = Settings {
        matters_root: matters_root.clone(),
        refused_trees: Vec::new(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
    };
    let new = NewMatter {
        id,
        folder,
        created_at: at(0),
        marker: DEFAULT_MARKER,
        checkpoint: cid(1),
    };
    let cancel = AtomicBool::new(false);
    let mut matter = match Matter::import(settings, new, &[], &mut |_| {}, &cancel).unwrap() {
        Imported::Done { matter, .. } => *matter,
        Imported::Cancelled => panic!("cancelled"),
    };

    let commit = matter.commit_of(cid(1)).unwrap();
    let (fan, rest) = commit.as_str().split_at(2);
    let object = matters_root
        .join(id.to_string())
        .join("history.git")
        .join("objects")
        .join(fan)
        .join(rest);
    assert!(object.is_file(), "{}", object.display());
    assert!(units(&object) > 300, "{}", units(&object));

    let before = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(before.outcome, CheckpointOutcome::Reused(cid(1)));
    replace(&lease, "second draft\n");
    let tools: BTreeSet<MatterPath> = [MatterPath::new("lease.txt").unwrap()].into();
    let after = matter.after_turn(&meta(3), &tools).unwrap();
    assert_eq!(after.outcome, CheckpointOutcome::Created(cid(3)));
    let ids: Vec<CheckpointId> = matter.timeline().iter().map(|entry| entry.id).collect();
    assert_eq!(ids, [cid(1), cid(3)]);

    let decisions = [FileDecision {
        path: MatterPath::new("lease.txt").unwrap(),
        choice: FileChoice::Undo,
    }];
    let undone = matter
        .undo(&UndoRequest {
            before: cid(1),
            after: cid(3),
            decisions: &decisions,
            thread: Some(meta(3).thread),
            turn: Some(meta(3).turn),
            outside_edits: cid(4),
            checkpoint: cid(5),
            at: at(5),
        })
        .unwrap();
    assert_eq!(undone.restored, [MatterPath::new("lease.txt").unwrap()]);
    assert_eq!(std::fs::read_to_string(&lease).unwrap(), "first draft\n");

    let restored = matter
        .restore(&Restore {
            from: cid(3),
            files: RestoreFiles::All,
            outside_edits: cid(6),
            checkpoint: cid(7),
            at: at(7),
        })
        .unwrap();
    assert_eq!(restored.written, [MatterPath::new("lease.txt").unwrap()]);
    assert_eq!(std::fs::read_to_string(&lease).unwrap(), "second draft\n");
    let ids: Vec<CheckpointId> = matter.timeline().iter().map(|entry| entry.id).collect();
    assert_eq!(ids, [cid(1), cid(3), cid(5), cid(7)]);
}

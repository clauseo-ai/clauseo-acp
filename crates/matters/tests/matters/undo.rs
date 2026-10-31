//! Undoing a step: restore and delete, edits made since, "Undo anyway", undoing an undo, and
//! the lock check.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use clauseo_matters::{
    CheckpointOutcome, Error, Imported, Limits, Matter, PlaceholderProbe, Settings, TimelineEntry,
};
use clauseo_protocol::{CheckpointKind, ErrorCode, FileChoice, FileDecision, SkippedFileReason};

use crate::fixture::{
    Fixture, added, assert_no_temp_files, assert_none, cid, created, deleted, meta, modified, path,
    thread, tools, turn, undo, undo_all, undo_request,
};

/// The step every test undoes: checkpoint 3 after a turn that modified `a.txt`, created
/// `c.txt` and deleted `b.txt`; checkpoint 1 before it.
fn stepped(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("a.txt", "one");
    fx.write("b.txt", "bee");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    fx.write("c.txt", "sea");
    crate::support::remove_file(fx.file("b.txt"));
    let tool_paths = tools(&STEP);
    created(matter.after_turn(&meta(3), &tool_paths).unwrap(), 3);
    (fx, matter)
}

const STEP: [&str; 3] = ["a.txt", "b.txt", "c.txt"];

fn kinds(matter: &Matter) -> Vec<CheckpointKind> {
    matter.timeline().iter().map(|entry| entry.kind).collect()
}

#[test]
fn undo_restores_before_contents_and_deletes_created() {
    let (fx, mut matter) = stepped("undo-restores");

    // A missing decision fails before anything is written.
    let err = undo(&mut matter, (1, 3), &["a.txt", "b.txt"], (4, 5)).unwrap_err();
    assert!(matches!(&err, Error::MissingDecision { path: p } if *p == path("c.txt")));
    assert_eq!(fx.read("a.txt"), b"two");
    assert_eq!(matter.timeline().len(), 2);

    let undone = undo(&mut matter, (1, 3), &STEP, (4, 5)).unwrap();
    assert_eq!(undone.outside_edits, None);
    assert_eq!(
        undone.restored,
        [path("a.txt"), path("b.txt"), path("c.txt")]
    );
    assert_none(&undone.kept);
    assert_none(&undone.edited_since);
    assert_eq!(
        created(undone.checkpoint, 5),
        [
            modified("a.txt", b"two", b"one"),
            added("b.txt", b"bee"),
            deleted("c.txt", b"sea"),
        ]
    );
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(fx.read("b.txt"), b"bee");
    assert!(!fx.exists("c.txt"));
    assert_no_temp_files(&fx.folder);
    assert_eq!(
        matter.timeline().last(),
        Some(&TimelineEntry {
            id: cid(5),
            kind: CheckpointKind::Undo,
            thread: Some(thread()),
            turn: Some(turn(3)),
        })
    );
}

#[test]
fn undo_keep_decision_leaves_file() {
    let (fx, mut matter) = stepped("undo-keep");
    let mut decisions = undo_all(&STEP);
    decisions[2].choice = clauseo_protocol::FileChoice::Keep;
    let undone = matter.undo(&undo_request(1, 3, &decisions, 4, 5)).unwrap();
    assert_eq!(undone.restored, [path("a.txt"), path("b.txt")]);
    assert_eq!(undone.kept, [path("c.txt")]);
    assert_eq!(fx.read("c.txt"), b"sea");
}

#[test]
fn undo_lists_files_edited_since() {
    let (fx, mut matter) = stepped("undo-edited-since");
    assert_none(&matter.edited_since(cid(1), cid(3)).unwrap());

    fx.write("a.txt", "the lawyer's edit");
    fx.write("b.txt", "the lawyer recreated it");
    assert_eq!(
        matter.edited_since(cid(1), cid(3)).unwrap(),
        [path("a.txt"), path("b.txt")]
    );

    let undone = undo(&mut matter, (1, 3), &STEP, (4, 5)).unwrap();
    assert_eq!(undone.edited_since, [path("a.txt"), path("b.txt")]);
    assert!(undone.outside_edits.is_some());
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(fx.read("b.txt"), b"bee");
}

#[test]
fn undo_anyway_keeps_overwritten_edits_in_versions() {
    let (fx, mut matter) = stepped("undo-anyway");
    fx.write("a.txt", "lawyer");

    let undone = undo(&mut matter, (1, 3), &STEP, (4, 5)).unwrap();
    let outside = undone.outside_edits.expect("an outside-edits checkpoint");
    assert_eq!(outside.outcome, CheckpointOutcome::Created(cid(4)));
    assert_eq!(outside.changes, [modified("a.txt", b"two", b"lawyer")]);
    assert!(!outside.changes[0].may_include_lawyer_edits());
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(
        created(undone.checkpoint, 5)[0],
        modified("a.txt", b"lawyer", b"one")
    );
    assert_eq!(
        kinds(&matter),
        [
            CheckpointKind::Import,
            CheckpointKind::AfterTurn,
            CheckpointKind::OutsideEdits,
            CheckpointKind::Undo,
        ]
    );
    let entry = matter.timeline()[2];
    assert_eq!((entry.thread, entry.turn), (None, None));
    // The lawyer's edit stays reachable: undoing the undo puts it back.
    let back = undo(&mut matter, (4, 5), &STEP, (6, 7)).unwrap();
    assert!(back.outside_edits.is_none());
    assert_eq!(fx.read("a.txt"), b"lawyer");
}

#[test]
fn undo_of_undo_brings_step_back() {
    let (fx, mut matter) = stepped("undo-of-undo");
    let undone = undo(&mut matter, (1, 3), &STEP, (4, 5)).unwrap();
    assert!(undone.outside_edits.is_none());

    // No outside edits: the checkpoint before the undo is the step's own after (3).
    let redone = undo(&mut matter, (3, 5), &STEP, (6, 7)).unwrap();
    assert_eq!(
        redone.restored,
        [path("a.txt"), path("b.txt"), path("c.txt")]
    );
    assert_eq!(
        created(redone.checkpoint, 7),
        [
            modified("a.txt", b"one", b"two"),
            deleted("b.txt", b"bee"),
            added("c.txt", b"sea"),
        ]
    );
    assert_eq!(fx.read("a.txt"), b"two");
    assert!(!fx.exists("b.txt"));
    assert_eq!(fx.read("c.txt"), b"sea");
    assert_eq!(
        kinds(&matter),
        [
            CheckpointKind::Import,
            CheckpointKind::AfterTurn,
            CheckpointKind::Undo,
            CheckpointKind::Undo,
        ]
    );
}

#[test]
fn undo_refuses_locked_file_and_writes_nothing() {
    let (fx, mut matter) = stepped("undo-locked");
    fx.write("~$a.txt", "owner file: open in Word");
    fx.write("unrelated.txt", "an outside edit");

    let err = undo(&mut matter, (1, 3), &STEP, (4, 5)).unwrap_err();
    assert!(
        matches!(&err, Error::FileLocked { paths } if *paths == [path("a.txt")]),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::MatterFileLocked);
    assert_eq!(fx.read("a.txt"), b"two");
    assert!(!fx.exists("b.txt"));
    assert_eq!(fx.read("c.txt"), b"sea");
    assert_eq!(matter.timeline().len(), 2);
    assert_eq!(matter.last_checkpoint(), Some(cid(3)));

    // Closed in Word: the undo goes through.
    crate::support::remove_file(fx.file("~$a.txt"));
    let undone = undo(&mut matter, (1, 3), &STEP, (4, 5)).unwrap();
    assert_eq!(
        created(undone.outside_edits.unwrap(), 4),
        [added("unrelated.txt", b"an outside edit")]
    );
    assert_eq!(fx.read("a.txt"), b"one");
}

#[test]
fn undo_recreates_deleted_subfolder() {
    let fx = Fixture::new("undo-subfolder");
    fx.write("sub/deep/a.txt", "deep");
    fx.write("sub/b.txt", "shallow");
    fx.write("top.txt", "top");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    crate::support::remove_dir_all(fx.file("sub"));
    fx.write("top.txt", "top, edited");
    let paths = ["sub/b.txt", "sub/deep/a.txt", "top.txt"];
    let changes = created(matter.after_turn(&meta(3), &tools(&paths)).unwrap(), 3);
    assert_eq!(changes.len(), 3);

    let undone = undo(&mut matter, (1, 3), &paths, (4, 5)).unwrap();
    assert_eq!(undone.restored.len(), 3);
    assert_eq!(fx.read("sub/deep/a.txt"), b"deep");
    assert_eq!(fx.read("sub/b.txt"), b"shallow");
    assert_eq!(fx.read("top.txt"), b"top");
    assert_no_temp_files(&fx.folder);
}

#[test]
fn undo_after_file_grew_over_cap_needs_no_decision_for_it() {
    let mut fx = Fixture::new("undo-grew-over-cap");
    fx.limits = Limits {
        file_cap_bytes: 10,
        import_warn_bytes: 1_000_000,
    };
    fx.write("big.pdf", "small");
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    let grown = [b'%'; 40];
    fx.write("big.pdf", grown);
    fx.write("a.txt", "two");
    let mtime = fx.file("big.pdf").metadata().unwrap().modified().unwrap();
    let checkpoint = matter
        .after_turn(&meta(3), &tools(&["a.txt", "big.pdf"]))
        .unwrap();
    assert_eq!(checkpoint.skipped.len(), 1);
    assert_eq!(checkpoint.skipped[0].path, path("big.pdf"));
    assert_eq!(checkpoint.skipped[0].reason, SkippedFileReason::OverSizeCap);
    let changes = created(checkpoint, 3);
    assert_eq!(changes, [modified("a.txt", b"one", b"two")]);

    // Decisions only for what the checkpoint reported.
    let decisions: Vec<FileDecision> = changes
        .iter()
        .map(|change| FileDecision {
            path: change.path().clone(),
            choice: FileChoice::Undo,
        })
        .collect();
    let undone = matter.undo(&undo_request(1, 3, &decisions, 4, 5)).unwrap();
    assert_eq!(undone.restored, [path("a.txt")]);
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(fx.read("big.pdf"), grown);
    let after = fx.file("big.pdf").metadata().unwrap().modified().unwrap();
    assert_eq!(after, mtime);
    assert_no_temp_files(&fx.folder);
}

/// Once armed, sets `target`'s mtime the `nth` time it is asked about it: a write landing
/// after the walk took the file's stat. Never cloud-only.
pub(crate) struct TouchingProbe {
    pub(crate) target: PathBuf,
    pub(crate) nth: usize,
    pub(crate) armed: AtomicBool,
    pub(crate) seen: AtomicUsize,
}

impl PlaceholderProbe for TouchingProbe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        if path == self.target
            && self.armed.load(Ordering::Relaxed)
            && self.seen.fetch_add(1, Ordering::Relaxed) + 1 == self.nth
        {
            let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
            crate::support::set_mtime(path, mtime);
        }
        false
    }
}

#[test]
fn undo_refuses_a_target_written_after_its_outside_edits_checkpoint() {
    let fx = Fixture::new("undo-moved-since");
    // Asked about a.txt once by undo's writable check, then by the outside-edits walk.
    let probe = Arc::new(TouchingProbe {
        target: fx.file("a.txt"),
        nth: 2,
        armed: AtomicBool::new(false),
        seen: AtomicUsize::new(0),
    });
    let settings = Settings {
        probe: Arc::<TouchingProbe>::clone(&probe),
        ..fx.settings()
    };
    fx.write("a.txt", "one");
    fx.write("b.txt", "bee");
    let cancel = AtomicBool::new(false);
    let imported = Matter::import(settings, fx.new_matter(), &[], &mut |_| {}, &cancel);
    let Ok(Imported::Done { matter, .. }) = imported else {
        panic!("{imported:?}");
    };
    let mut matter = *matter;
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    fx.write("c.txt", "sea");
    crate::support::remove_file(fx.file("b.txt"));
    created(matter.after_turn(&meta(3), &tools(&STEP)).unwrap(), 3);

    probe.armed.store(true, Ordering::Relaxed);
    let err = undo(&mut matter, (1, 3), &STEP, (4, 5)).unwrap_err();
    assert!(
        matches!(&err, Error::FileLocked { paths } if *paths == [path("a.txt")]),
        "{err:?}"
    );
    assert_eq!(probe.seen.load(Ordering::Relaxed), 2);
    assert_eq!(fx.read("a.txt"), b"two");
    assert!(!fx.exists("b.txt"));
    assert_eq!(fx.read("c.txt"), b"sea");
    assert_no_temp_files(&fx.folder);
}

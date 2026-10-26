//! `before_turn` and `after_turn`: reuse, the diff, the lawyer-edits flag, the exclusion list and
//! racily clean files.

use std::time::SystemTime;

use clauseo_matters::{CheckpointOutcome, Limits, TimelineEntry};
use clauseo_protocol::CheckpointKind;

use crate::fixture::{
    Fixture, added, assert_none, cid, created, deleted, meta, modified, renamed, thread, tools,
    turn,
};
use crate::support;

#[test]
fn before_turn_reuses_last_checkpoint_when_unchanged() {
    let fx = Fixture::new("before-reuse");
    fx.write("a.txt", "one");
    let mut matter = fx.import();

    let checkpoint = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_none(&checkpoint.changes);
    assert_none(&checkpoint.skipped);
    assert_eq!(matter.timeline().len(), 1);

    // An edit outside Clauseo between turns: the next before-turn checkpoint records it.
    fx.write("a.txt", "outside edit");
    let changes = created(matter.before_turn(&meta(3)).unwrap(), 3);
    assert_eq!(changes, [modified("a.txt", b"one", b"outside edit")]);
    assert!(!changes[0].may_include_lawyer_edits());
    assert_eq!(
        matter.timeline().last(),
        Some(&TimelineEntry {
            id: cid(3),
            kind: CheckpointKind::BeforeTurn,
            thread: Some(thread()),
            turn: Some(turn(3)),
        })
    );
    assert_eq!(matter.last_checkpoint(), Some(cid(3)));
}

#[test]
fn after_turn_lists_files_changed() {
    let fx = Fixture::new("after-changes");
    fx.write("keep.txt", "kept");
    fx.write("mod.txt", "before");
    fx.write("del.txt", "doomed");
    fx.write("old-name.txt", "moving bytes");
    let mut matter = fx.import();
    assert_eq!(matter.before_turn(&meta(2)).unwrap().outcome.id(), cid(1));

    fx.write("new.txt", "fresh");
    fx.write("mod.txt", "after, longer");
    support::remove_file(fx.file("del.txt"));
    support::create_dir_all(fx.file("sub"));
    support::rename(fx.file("old-name.txt"), fx.file("sub/new-name.txt"));
    let all = [
        "new.txt",
        "mod.txt",
        "del.txt",
        "old-name.txt",
        "sub/new-name.txt",
    ];
    let changes = created(matter.after_turn(&meta(3), &tools(&all)).unwrap(), 3);
    assert_eq!(
        changes,
        [
            deleted("del.txt", b"doomed"),
            modified("mod.txt", b"before", b"after, longer"),
            added("new.txt", b"fresh"),
            renamed("old-name.txt", "sub/new-name.txt", b"moving bytes"),
        ]
    );
    assert_eq!(changes[0].size_bytes().get(), 6);
    assert_eq!(changes[1].size_bytes().get(), 13);
    assert_eq!(matter.timeline()[1].kind, CheckpointKind::AfterTurn);

    // A turn that changes nothing: both its checkpoints are the last one.
    let before = matter.before_turn(&meta(4)).unwrap();
    assert_eq!(before.outcome, CheckpointOutcome::Reused(cid(3)));
    let after = matter.after_turn(&meta(5), &tools(&[])).unwrap();
    assert_eq!(
        after.outcome,
        CheckpointOutcome::Reused(before.outcome.id())
    );
    assert_none(&after.changes);
    assert_eq!(matter.timeline().len(), 2);
}

#[test]
fn may_include_lawyer_edits_without_tool_call() {
    let fx = Fixture::new("lawyer-edits");
    fx.write("agent.txt", "a");
    fx.write("lawyer.txt", "l");
    fx.write("from.txt", "renamed by the agent");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();

    fx.write("agent.txt", "agent wrote this");
    fx.write("lawyer.txt", "lawyer wrote this");
    support::rename(fx.file("from.txt"), fx.file("to.txt"));
    // The tool call named the rename's old path only.
    let tool_paths = tools(&["agent.txt", "from.txt"]);
    let changes = created(matter.after_turn(&meta(3), &tool_paths).unwrap(), 3);
    let flags: Vec<(&str, bool)> = changes
        .iter()
        .map(|change| (change.path().as_str(), change.may_include_lawyer_edits()))
        .collect();
    assert_eq!(
        flags,
        [
            ("agent.txt", false),
            ("lawyer.txt", true),
            ("to.txt", false)
        ]
    );
    assert_eq!(
        changes[1],
        modified("lawyer.txt", b"l", b"lawyer wrote this").with_lawyer_edits()
    );

    // With no tool call at all, every change is flagged.
    fx.write("agent.txt", "again");
    let changes = created(matter.after_turn(&meta(4), &tools(&[])).unwrap(), 4);
    assert!(
        changes
            .iter()
            .all(clauseo_protocol::FileChange::may_include_lawyer_edits)
    );
}

/// Lock and system files, each spelling in its own folder (macOS folders ignore case).
const EXCLUDED: [&str; 6] = [
    "~$x.docx",
    "~WRL0001.tmp",
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    ".~lock.a.odt#",
];

fn spellings(name: &str) -> [String; 3] {
    [
        name.to_owned(),
        name.to_ascii_lowercase(),
        name.to_ascii_uppercase(),
    ]
}

#[test]
fn office_lock_files_never_versioned() {
    let mut fx = Fixture::new("excluded-names");
    // Each excluded file is over this cap, so it would be listed as skipped if it were seen.
    fx.limits = Limits {
        file_cap_bytes: 100,
        import_warn_bytes: 1_000_000,
    };
    fx.write("doc.docx", "real");
    for name in EXCLUDED {
        for (n, spelling) in spellings(name).iter().enumerate() {
            fx.write(&format!("v{n}/{spelling}"), [0; 200]);
        }
    }
    let (mut matter, summary) = fx.import_with_summary();
    assert_eq!(summary.files, 1);
    let checkpoint = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_none(&checkpoint.skipped);

    // New ones appear and the old ones change during a turn: only the document is listed.
    fx.write("doc.docx", "real, edited");
    for name in EXCLUDED {
        for (n, spelling) in spellings(name).iter().enumerate() {
            fx.write(&format!("v{n}/{spelling}"), [1; 300]);
            fx.write(&format!("new/v{n}/{spelling}"), b"small");
        }
    }
    let checkpoint = matter.after_turn(&meta(3), &tools(&["doc.docx"])).unwrap();
    assert_none(&checkpoint.skipped);
    assert_eq!(
        created(checkpoint, 3),
        [modified("doc.docx", b"real", b"real, edited")]
    );
}

#[test]
fn same_size_rewrite_in_same_second_is_seen() {
    let fx = Fixture::new("racy");
    fx.write("a.txt", "aaaa");
    let mut matter = fx.import();
    let old: SystemTime = fx.file("a.txt").metadata().unwrap().modified().unwrap();
    assert_eq!(matter.before_turn(&meta(2)).unwrap().outcome.id(), cid(1));

    fx.write("a.txt", "bbbb");
    support::set_mtime(fx.file("a.txt"), old);
    assert_eq!(
        fx.file("a.txt").metadata().unwrap().modified().unwrap(),
        old
    );
    let changes = created(matter.after_turn(&meta(3), &tools(&["a.txt"])).unwrap(), 3);
    assert_eq!(changes, [modified("a.txt", b"aaaa", b"bbbb")]);
}

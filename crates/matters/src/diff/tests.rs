use std::collections::BTreeSet;

use clauseo_protocol::{ChangeKind, MatterPath, SkippedFile, SkippedFileReason, Timestamp};

use super::{TurnStart, diff, flag};
use crate::checkpoint::SizedFile;
use crate::first_read::FirstReads;
use crate::history::Oid;
use crate::skip_record::{SkipEntry, SkipRecord};
use crate::stat::{CacheEntry, EntryState, FileTime, Stat};

fn file(path: &str, blob: u8) -> SizedFile {
    SizedFile {
        path: MatterPath::new(path).unwrap(),
        blob: Oid([blob; 20]),
        size: u64::from(blob),
    }
}

fn kinds(changes: &[clauseo_protocol::FileChange]) -> Vec<(String, ChangeKind)> {
    changes
        .iter()
        .map(|change| (change.path().as_str().to_owned(), change.change().clone()))
        .collect()
}

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

#[test]
fn diff_lists_added_modified_and_deleted_files_sorted_by_path() {
    let old = [file("a.docx", 1), file("b.docx", 2), file("c.docx", 3)];
    let new = [file("a.docx", 1), file("b.docx", 4), file("d.docx", 5)];
    let changes = diff(&old, &new, &BTreeSet::new()).unwrap();
    assert_eq!(
        kinds(&changes),
        [
            ("b.docx".to_owned(), ChangeKind::Modified),
            ("c.docx".to_owned(), ChangeKind::Deleted),
            ("d.docx".to_owned(), ChangeKind::Added),
        ]
    );
    let modified = &changes[0];
    assert_eq!(modified.before().unwrap().as_str(), "02".repeat(20));
    assert_eq!(modified.after().unwrap().as_str(), "04".repeat(20));
    assert_eq!(modified.size_bytes().get(), 4);
    // A deleted file's size is its size before.
    assert_eq!(changes[1].size_bytes().get(), 3);
}

#[test]
fn a_delete_and_an_add_with_the_same_blob_are_a_rename_paired_in_path_order() {
    let old = [file("a.docx", 1), file("b.docx", 1), file("keep.docx", 9)];
    let new = [
        file("keep.docx", 9),
        file("x.docx", 1),
        file("y.docx", 1),
        file("z.docx", 1),
    ];
    let changes = diff(&old, &new, &BTreeSet::new()).unwrap();
    assert_eq!(
        kinds(&changes),
        [
            (
                "x.docx".to_owned(),
                ChangeKind::Renamed {
                    from: path("a.docx")
                }
            ),
            (
                "y.docx".to_owned(),
                ChangeKind::Renamed {
                    from: path("b.docx")
                }
            ),
            ("z.docx".to_owned(), ChangeKind::Added),
        ]
    );
}

#[test]
fn a_file_that_grew_over_the_cap_is_not_a_deletion() {
    let old = [file("big.pdf", 1), file("gone.pdf", 2)];
    let over_cap = BTreeSet::from(["big.pdf"]);
    let changes = diff(&old, &[], &over_cap).unwrap();
    assert_eq!(
        kinds(&changes),
        [("gone.pdf".to_owned(), ChangeKind::Deleted)]
    );
}

#[test]
fn only_changes_no_tool_call_named_are_flagged() {
    let old = [file("a.docx", 1), file("b.docx", 2), file("old.docx", 3)];
    let new = [file("a.docx", 4), file("b.docx", 5), file("new.docx", 3)];
    let tools = BTreeSet::from([path("a.docx"), path("old.docx")]);
    // No start record: no added file is known as the agent's.
    let start = TurnStart {
        record: None,
        tree: &old,
        reads: &FirstReads::default(),
        entries: &[],
        at: Timestamp::ZERO,
    };
    let flagged: Vec<(String, bool)> = diff(&old, &new, &BTreeSet::new())
        .unwrap()
        .into_iter()
        .map(|change| flag(change, &tools, &start))
        .map(|change| {
            (
                change.path().as_str().to_owned(),
                change.may_include_lawyer_edits(),
            )
        })
        .collect();
    assert_eq!(
        flagged,
        [
            ("a.docx".to_owned(), false),
            ("b.docx".to_owned(), true),
            ("new.docx".to_owned(), false),
        ]
    );
}

/// An entry read at `path` last modified at `secs`.
fn entry(path_: &str, blob: u8, secs: u32) -> CacheEntry {
    CacheEntry {
        path: path(path_),
        stat: Stat {
            mtime: FileTime { secs, nanos: 0 },
            ..Stat::default()
        },
        state: EntryState::Read(Oid([blob; 20])),
    }
}

/// Whether the added `made.docx` (blob 9, modified at 100 s, the turn ending at 100 s) comes out
/// flagged, with no tool call naming it, given the start `record` and first `reads`.
fn made_flagged(record: &SkipRecord, reads: &FirstReads, secs: u32) -> bool {
    let old = [file("a.docx", 1)];
    let new = [file("a.docx", 1), file("made.docx", 9)];
    let entries = [entry("a.docx", 1, 50), entry("made.docx", 9, secs)];
    let start = TurnStart {
        record: Some(record),
        tree: &old,
        reads,
        entries: &entries,
        at: Timestamp::new(100_000).unwrap(),
    };
    let changes = diff(&old, &new, &BTreeSet::new()).unwrap();
    assert_eq!(changes.len(), 1);
    let change = changes.into_iter().next().unwrap();
    flag(change, &BTreeSet::new(), &start).may_include_lawyer_edits()
}

/// D-sweep: a file the turn added that its start record proves absent, untouched since the turn
/// ended, is the agent's even with no tool call naming it; any doubt keeps the flag.
#[test]
fn an_added_file_proved_absent_at_the_start_is_the_agents() {
    let clean = SkipRecord::default();
    let none = FirstReads::default();
    assert!(!made_flagged(&clean, &none, 100));
    // Modified after the turn's end.
    assert!(made_flagged(&clean, &none, 101));
    // A skipped file there at the start, or a folder that did not list.
    let skipped = SkipRecord {
        skipped: vec![SkipEntry {
            path: path("made.docx"),
            reason: SkippedFileReason::CloudOnly,
            size: 9,
            mtime: FileTime::default(),
        }],
        ..SkipRecord::default()
    };
    assert!(made_flagged(&skipped, &none, 100));
    let unread = SkipRecord {
        unread: vec!["Sub".to_owned()],
        ..SkipRecord::default()
    };
    assert!(made_flagged(&unread, &none, 100));
    // Read for the first time: a skipped file moved there.
    let moved = FirstReads {
        changed: vec![SkippedFile {
            path: path("made.docx"),
            reason: SkippedFileReason::OverSizeCap,
        }],
        ..FirstReads::default()
    };
    assert!(made_flagged(&clean, &moved, 100));
}

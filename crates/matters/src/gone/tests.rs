use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason};

use super::{gone, no_copy};
use crate::history::Oid;
use crate::skip_record::{SkipEntry, SkipRecord};
use crate::stat::{CacheEntry, EntryState, FileTime, Stat};

const CLOUD: SkippedFileReason = SkippedFileReason::CloudOnly;
const LARGE: SkippedFileReason = SkippedFileReason::OverSizeCap;

fn entry(path: &str, size: u32, mtime: u32, state: EntryState) -> CacheEntry {
    CacheEntry {
        path: MatterPath::new(path).unwrap(),
        stat: Stat {
            size,
            mtime: FileTime {
                secs: mtime,
                nanos: 0,
            },
            ..Stat::default()
        },
        state,
    }
}

fn skip(path: &str, size: u32, mtime: u32, reason: SkippedFileReason) -> SkipEntry {
    SkipEntry {
        path: MatterPath::new(path).unwrap(),
        reason,
        size,
        mtime: FileTime {
            secs: mtime,
            nanos: 0,
        },
    }
}

fn file(path: &str, reason: SkippedFileReason) -> SkippedFile {
    SkippedFile {
        path: MatterPath::new(path).unwrap(),
        reason,
    }
}

fn record(skipped: Vec<SkipEntry>, unread: &[&str]) -> SkipRecord {
    SkipRecord {
        skipped,
        stale: Vec::new(),
        unread: unread.iter().map(|folder| (*folder).to_owned()).collect(),
    }
}

fn read() -> EntryState {
    EntryState::Read(Oid([1; 20]))
}

fn none(_: &str) -> bool {
    false
}

#[test]
fn a_skipped_file_the_walk_no_longer_finds_is_gone_with_its_reason() {
    let start = record(
        vec![
            skip("Brief.docx", 18, 100, CLOUD),
            skip("Scan.tiff", 900, 100, LARGE),
            skip("Still.pdf", 5, 100, CLOUD),
        ],
        &[],
    );
    let entries = vec![entry("Still.pdf", 5, 100, EntryState::Skipped)];
    let now = record(vec![skip("Still.pdf", 5, 100, CLOUD)], &[]);
    assert_eq!(
        gone(&start, &entries, &now, &none),
        vec![file("Brief.docx", CLOUD), file("Scan.tiff", LARGE)]
    );
}

#[test]
fn a_skipped_file_read_now_or_still_skipped_is_not_gone() {
    let start = record(
        vec![
            skip("a.docx", 18, 100, CLOUD),
            skip("b.docx", 18, 100, CLOUD),
        ],
        &[],
    );
    // a.docx downloaded and read; b.docx changed but still cloud-only.
    let entries = vec![
        entry("a.docx", 18, 100, read()),
        entry("b.docx", 30, 200, EntryState::Skipped),
    ];
    assert_eq!(gone(&start, &entries, &SkipRecord::default(), &none), []);
}

#[test]
fn a_skipped_file_under_a_folder_that_did_not_list_is_not_gone() {
    let start = record(vec![skip("Sub/x.docx", 18, 100, CLOUD)], &[]);
    let now = record(Vec::new(), &["Sub"]);
    assert_eq!(gone(&start, &[], &now, &none), []);
    // A folder whose name only starts the same hides nothing.
    let now = record(Vec::new(), &["Su"]);
    assert_eq!(
        gone(&start, &[], &now, &none),
        vec![file("Sub/x.docx", CLOUD)]
    );
}

#[test]
fn a_new_file_with_a_gone_files_size_and_mtime_is_that_file_moved() {
    let start = record(vec![skip("Old.docx", 18, 100, CLOUD)], &[]);
    let moved = vec![entry("Archive/Old.docx", 18, 100, EntryState::Skipped)];
    assert_eq!(gone(&start, &moved, &SkipRecord::default(), &none), []);
    let read_moved = vec![entry("New.docx", 18, 100, read())];
    assert_eq!(gone(&start, &read_moved, &SkipRecord::default(), &none), []);
    // Another stat is another file.
    let other = vec![entry("New.docx", 19, 100, read())];
    assert_eq!(
        gone(&start, &other, &SkipRecord::default(), &none),
        vec![file("Old.docx", CLOUD)]
    );
}

#[test]
fn a_file_that_was_there_before_never_counts_as_the_gone_one_moved() {
    let start = record(
        vec![
            skip("Gone.docx", 18, 100, CLOUD),
            skip("Kept.docx", 18, 100, CLOUD),
        ],
        &[],
    );
    // Kept.docx (skipped before) and Versioned.txt (in the tree) share the stat but were there.
    let entries = vec![
        entry("Kept.docx", 18, 100, EntryState::Skipped),
        entry("Versioned.txt", 18, 100, read()),
    ];
    let versioned = |path: &str| path == "Versioned.txt";
    assert_eq!(
        gone(&start, &entries, &SkipRecord::default(), &versioned),
        vec![file("Gone.docx", CLOUD)]
    );
}

#[test]
fn nothing_skipped_before_is_nothing_gone() {
    let entries = vec![entry("a.txt", 1, 1, read())];
    assert_eq!(
        gone(
            &SkipRecord::default(),
            &entries,
            &SkipRecord::default(),
            &none
        ),
        []
    );
}

#[test]
fn every_skipped_file_is_one_without_a_copy_and_a_stale_one_is_not() {
    let mut now = record(
        vec![
            skip("Big.zip", 18, 100, LARGE),
            skip("Scan.pdf", 18, 100, CLOUD),
        ],
        &["Unread"],
    );
    now.stale = vec![skip("Lease.docx", 18, 100, CLOUD)];
    assert_eq!(
        no_copy(&now),
        vec![file("Big.zip", LARGE), file("Scan.pdf", CLOUD)]
    );
    assert_eq!(no_copy(&SkipRecord::default()), []);
}

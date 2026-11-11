use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason};

use super::{FirstReads, StepReads, first_reads};
use crate::history::Oid;
use crate::skip_record::{SkipEntry, SkipRecord, SkipsRef};
use crate::stat::{CacheEntry, EntryState, FileTime, Stat};

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

fn file(path: &str, reason: SkippedFileReason) -> SkippedFile {
    SkippedFile {
        path: MatterPath::new(path).unwrap(),
        reason,
    }
}

fn blob(byte: u8) -> Oid {
    Oid([byte; 20])
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

#[test]
fn a_skipped_file_read_with_its_size_and_mtime_is_a_baseline_and_otherwise_changed() {
    let cloud = SkippedFileReason::CloudOnly;
    let read = EntryState::Read(blob(1));
    let old = SkipRecord {
        skipped: vec![
            skip("a.txt", 5, 100, cloud),
            skip("b.txt", 5, 100, cloud),
            skip("big.bin", 50, 100, SkippedFileReason::OverSizeCap),
            skip("c.txt", 5, 100, cloud),
            skip("gone.txt", 7, 300, cloud),
        ],
        stale: Vec::new(),
        unread: Vec::new(),
    };
    let new = [
        // Read unchanged: a baseline.
        entry("a.txt", 5, 100, read),
        // Another mtime, then (below) another size: changed.
        entry("b.txt", 5, 101, read),
        entry("big.bin", 8, 100, read),
        // Still skipped: not read for the first time.
        entry("c.txt", 5, 100, EntryState::Skipped),
        // Versioned before, with the stat of the skipped file now gone: not a move.
        entry("d.txt", 7, 300, read),
        // New, with the stat of the skipped file now gone: that file moved.
        entry("moved/gone.txt", 7, 300, read),
        // New, another stat: added, not a first read.
        entry("new.txt", 7, 301, read),
    ];
    let versioned = |path: &str| path == "d.txt";
    assert_eq!(
        first_reads(&old, &new, &versioned),
        FirstReads {
            skips: SkipsRef::Unrecorded,
            baseline: vec![file("a.txt", cloud)],
            changed: vec![
                file("b.txt", cloud),
                file("big.bin", SkippedFileReason::OverSizeCap),
                file("moved/gone.txt", cloud),
            ],
        }
    );
    assert_eq!(
        first_reads(&SkipRecord::default(), &new, &versioned),
        FirstReads::default()
    );
}

#[test]
fn a_range_keeps_each_baseline_with_its_checkpoint_and_every_first_read() {
    let earlier = FirstReads {
        skips: SkipsRef::Empty,
        baseline: vec![file("a.txt", SkippedFileReason::OverSizeCap)],
        changed: vec![file("b.txt", SkippedFileReason::CloudOnly)],
    };
    let last = FirstReads {
        skips: SkipsRef::Empty,
        baseline: vec![
            file("d.txt", SkippedFileReason::CloudOnly),
            file("c.txt", SkippedFileReason::CloudOnly),
        ],
        changed: Vec::new(),
    };
    let (one, two) = (Oid([1; 20]), Oid([2; 20]));
    let step = StepReads::of([(&earlier, one), (&last, two)].into_iter());
    // Each baseline with the tree of the checkpoint that read it.
    let baselines: Vec<(&str, Oid)> = step
        .baseline
        .iter()
        .map(|(path, tree)| (path.as_str(), *tree))
        .collect();
    assert_eq!(baselines, [("a.txt", one), ("c.txt", two), ("d.txt", two)]);
    let there: Vec<(&str, SkippedFileReason)> = step
        .there
        .iter()
        .map(|(path, reason)| (path.as_str(), *reason))
        .collect();
    assert_eq!(
        there,
        [
            ("a.txt", SkippedFileReason::OverSizeCap),
            ("b.txt", SkippedFileReason::CloudOnly),
            ("c.txt", SkippedFileReason::CloudOnly),
            ("d.txt", SkippedFileReason::CloudOnly),
        ]
    );
    assert_eq!(StepReads::of(std::iter::empty()).there.len(), 0);
}

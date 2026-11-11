use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason};

use super::{FirstReads, StepReads, first_reads};
use crate::history::Oid;
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

#[test]
fn a_skipped_file_read_with_its_size_and_mtime_is_a_baseline_and_otherwise_changed() {
    let skipped = EntryState::Skipped;
    let read = EntryState::Read(blob(1));
    let old = [
        entry("a.txt", 5, 100, skipped),
        entry("b.txt", 5, 100, skipped),
        entry("big.bin", 50, 100, skipped),
        entry("c.txt", 5, 100, skipped),
        entry("d.txt", 5, 100, read),
        entry("e.txt", 5, 100, skipped),
    ];
    let new = [
        // Read unchanged: a baseline.
        entry("a.txt", 5, 100, read),
        // Another mtime, then (below) another size: changed.
        entry("b.txt", 5, 101, read),
        entry("big.bin", 8, 100, read),
        // Still skipped, or read before: not read for the first time.
        entry("c.txt", 5, 100, skipped),
        entry("d.txt", 6, 100, EntryState::Read(blob(2))),
        // Downloaded since it was versioned: kept as `Unread`, never a first read.
        entry("e.txt", 5, 100, EntryState::Unread(blob(3))),
        // New: added, not a first read.
        entry("f.txt", 5, 100, read),
    ];
    assert_eq!(
        first_reads(&old, &new, 10),
        FirstReads {
            baseline: vec![file("a.txt", SkippedFileReason::CloudOnly)],
            changed: vec![
                file("b.txt", SkippedFileReason::CloudOnly),
                file("big.bin", SkippedFileReason::OverSizeCap),
            ],
        }
    );
}

#[test]
fn a_step_drops_only_its_last_checkpoints_baselines_and_keeps_every_first_read() {
    let earlier = FirstReads {
        baseline: vec![file("a.txt", SkippedFileReason::OverSizeCap)],
        changed: vec![file("b.txt", SkippedFileReason::CloudOnly)],
    };
    let last = FirstReads {
        baseline: vec![
            file("d.txt", SkippedFileReason::CloudOnly),
            file("c.txt", SkippedFileReason::CloudOnly),
        ],
        changed: Vec::new(),
    };
    let step = StepReads::of([earlier, last].iter());
    assert!(step.is_baseline("c.txt") && step.is_baseline("d.txt"));
    assert!(!step.is_baseline("a.txt") && !step.is_baseline("b.txt"));
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
    assert!(StepReads::of(std::iter::empty()).there.is_empty());
}

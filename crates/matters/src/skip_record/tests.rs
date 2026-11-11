use clauseo_protocol::{MatterPath, SkippedFileReason};

use super::{SkipEntry, SkipRecord, reason_by_size};
use crate::stat::{CacheEntry, EntryState, FileTime, Stat};

fn entry(path: &str, size: u32, state: EntryState) -> CacheEntry {
    CacheEntry {
        path: MatterPath::new(path).unwrap(),
        stat: Stat {
            size,
            mtime: FileTime {
                secs: 1_700_000_000,
                nanos: 5,
            },
            ..Stat::default()
        },
        state,
    }
}

#[test]
fn a_record_round_trips_through_its_text() {
    let entries = [
        entry("big.bin", 900, EntryState::Skipped),
        entry(
            "brief.txt",
            5,
            EntryState::Read(crate::history::Oid([1; 20])),
        ),
        entry("notes 2.txt ", 18, EntryState::Skipped),
    ];
    let reasons = [("notes 2.txt ".to_owned(), SkippedFileReason::CloudOnly)]
        .into_iter()
        .collect();
    let record = SkipRecord::of_walk(&entries, &["Sub".to_owned()], &reasons, 100);
    let text = record.encode();
    assert_eq!(
        text,
        "clauseo-skips 1\n\
         unread Sub\n\
         skipped over-size-cap 900 1700000000.000000005 big.bin\n\
         skipped cloud-only 18 1700000000.000000005 notes 2.txt \n"
    );
    assert_eq!(SkipRecord::decode(&text), Some(record.clone()));
    assert_eq!(
        (record.is_empty(), SkipRecord::default().is_empty()),
        (false, true)
    );
    assert_eq!(
        record.skipped_at("notes 2.txt ").map(|skip| skip.reason),
        Some(SkippedFileReason::CloudOnly)
    );
    assert!(record.skipped_at("brief.txt").is_none());
}

#[test]
fn a_path_under_an_unread_folder_is_one_inside_it() {
    let record = SkipRecord {
        skipped: Vec::new(),
        stale: Vec::new(),
        unread: vec!["Sub".to_owned(), "a/b".to_owned()],
    };
    assert!(record.under_unread("Sub/x.txt"));
    assert!(record.under_unread("a/b/c/d.txt"));
    assert!(!record.under_unread("Sub"));
    assert!(!record.under_unread("Subway/x.txt"));
    assert!(!record.under_unread("a/bc/d.txt"));
}

#[test]
fn a_record_with_a_line_that_does_not_read_is_unreadable() {
    assert_eq!(SkipRecord::decode("clauseo-skips 2\nunread Sub\n"), None);
    // A reason it does not know is still a skipped file.
    let unknown = "clauseo-skips 1\nskipped too-deep 3 1.000000000 deep.txt\n";
    assert_eq!(
        SkipRecord::decode(unknown).map(|record| record.skipped),
        Some(vec![SkipEntry {
            path: MatterPath::new("deep.txt").unwrap(),
            reason: SkippedFileReason::Unknown,
            size: 3,
            mtime: FileTime { secs: 1, nanos: 0 },
        }])
    );
    for bad in [
        "renamed a b",
        "skipped cloud-only x 1.0 bad.txt",
        "stale 3 nowhen bad.txt",
        "unread /rooted",
        "",
    ] {
        let text = format!("clauseo-skips 1\nunread Sub\n{bad}\n");
        assert_eq!(SkipRecord::decode(&text), None, "{bad:?}");
    }
    // Every line ends with a line break, the last too.
    assert_eq!(SkipRecord::decode("clauseo-skips 1\nunread Sub"), None);
}

#[test]
fn a_reason_by_size_is_over_the_cap_only_past_it() {
    assert_eq!(reason_by_size(101, 100), SkippedFileReason::OverSizeCap);
    assert_eq!(reason_by_size(100, 100), SkippedFileReason::CloudOnly);
}

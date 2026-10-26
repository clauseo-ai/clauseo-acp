use clauseo_protocol::MatterPath;

use super::{CacheEntry, EntryState, FileTime, Stat, StatCache};
use crate::history::Oid;

fn stat(mtime_secs: u32) -> Stat {
    Stat {
        mtime: FileTime {
            secs: mtime_secs,
            nanos: 0,
        },
        size: 10,
        ..Stat::default()
    }
}

fn cache(state: EntryState, mtime_secs: u32, written_secs: Option<u32>) -> StatCache {
    StatCache {
        entries: vec![CacheEntry {
            path: MatterPath::new("a.docx").unwrap(),
            stat: stat(mtime_secs),
            state,
        }],
        written_at: written_secs.map(|secs| FileTime { secs, nanos: 0 }),
    }
}

const BLOB: Oid = Oid([7; 20]);

#[test]
fn an_unchanged_file_written_long_before_the_cache_is_clean() {
    let cache = cache(EntryState::Read(BLOB), 1_000, Some(1_002));
    assert_eq!(cache.clean_blob("a.docx", &stat(1_000)), Some(BLOB));
}

#[test]
fn a_file_modified_within_two_seconds_of_the_cache_write_is_read_again() {
    // 1.999 s before the write: racily clean, so a same-size rewrite in that window is seen.
    let mut racy = cache(EntryState::Read(BLOB), 1_000, Some(1_001));
    racy.written_at = Some(FileTime {
        secs: 1_001,
        nanos: 999_999_999,
    });
    assert_eq!(racy.clean_blob("a.docx", &stat(1_000)), None);
    // Modified after the write: racy too.
    let after = cache(EntryState::Read(BLOB), 1_005, Some(1_000));
    assert_eq!(after.clean_blob("a.docx", &stat(1_005)), None);
    // A cache never written vouches for nothing.
    let unwritten = cache(EntryState::Read(BLOB), 1_000, None);
    assert_eq!(unwritten.clean_blob("a.docx", &stat(1_000)), None);
}

#[test]
fn a_changed_stat_or_an_unread_or_skipped_entry_is_never_clean() {
    let read = cache(EntryState::Read(BLOB), 1_000, Some(5_000));
    let mut bigger = stat(1_000);
    bigger.size = 11;
    assert_eq!(read.clean_blob("a.docx", &bigger), None);
    assert_eq!(read.clean_blob("b.docx", &stat(1_000)), None);
    let unread = cache(EntryState::Unread(BLOB), 1_000, Some(5_000));
    assert_eq!(unread.clean_blob("a.docx", &stat(1_000)), None);
    let skipped = cache(EntryState::Skipped, 1_000, Some(5_000));
    assert_eq!(skipped.clean_blob("a.docx", &stat(1_000)), None);
}

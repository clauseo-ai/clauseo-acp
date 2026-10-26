//! The stat cache as the index file.

use clauseo_protocol::MatterPath;

use super::{new_store, write_file};
use crate::history::{HistoryStore, Oid};
use crate::stat::{CacheEntry, EntryState, FileTime, Stat, StatCache};
use crate::test_dir::TestDir;

fn entry(path: &str, secs: u32, state: EntryState) -> CacheEntry {
    let time = |secs| FileTime {
        secs,
        nanos: 123_456_789,
    };
    CacheEntry {
        path: MatterPath::new(path).unwrap(),
        stat: Stat {
            mtime: time(secs),
            ctime: time(secs + 1),
            dev: 7,
            ino: secs,
            size: 4096,
        },
        state,
    }
}

#[test]
fn the_cache_round_trips_all_three_states() {
    let dir = TestDir::new("history-cache-round-trip");
    let (git_dir, mut store) = new_store(&dir);
    let mut cache = StatCache {
        entries: vec![
            entry("Brief.docx", 10, EntryState::Read(Oid([1; 20]))),
            entry("cloud/Exhibit A.pdf", 20, EntryState::Unread(Oid([2; 20]))),
            entry("video.mov", 30, EntryState::Skipped),
        ],
        written_at: None,
    };
    store.save_cache(&mut cache).unwrap();
    let mtime = std::fs::metadata(git_dir.join("index"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(cache.written_at, Some(FileTime::from_system(mtime)));

    let bytes = std::fs::read(git_dir.join("index")).unwrap();
    assert_eq!(&bytes[..8], b"DIRC\0\0\0\x03", "not an index of version 3");
    assert_eq!(store.load_cache().unwrap(), Some(cache));
}

#[test]
fn a_missing_cache_loads_as_none() {
    let dir = TestDir::new("history-cache-missing");
    let (_git_dir, store) = new_store(&dir);
    assert_eq!(store.load_cache().unwrap(), None);
}

#[test]
fn a_garbage_cache_loads_as_none() {
    let dir = TestDir::new("history-cache-garbage");
    let (git_dir, store) = new_store(&dir);
    write_file(
        &git_dir.join("index"),
        b"DIRC\0\0\0\x02 this is not an index",
    );
    assert_eq!(store.load_cache().unwrap(), None);
}

#[test]
fn an_unsorted_cache_loads_as_none() {
    let dir = TestDir::new("history-cache-unsorted");
    let (_git_dir, mut store) = new_store(&dir);
    let mut cache = StatCache {
        entries: vec![
            entry("b.txt", 1, EntryState::Read(Oid([1; 20]))),
            entry("a.txt", 2, EntryState::Read(Oid([2; 20]))),
        ],
        written_at: None,
    };
    store.save_cache(&mut cache).unwrap();
    assert_eq!(store.load_cache().unwrap(), None);
}

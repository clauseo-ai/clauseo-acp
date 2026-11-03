//! Opening: what needs a rebuild, the empty index it then gets, the file mode, and progress.

use clauseo_search::{FILE_NAME, Mode, SearchIndex};

use crate::common::{TestDir, answer, count, find, index};
use crate::write;

fn set_meta(dir: &TestDir, key: &str, value: &str) {
    let conn = rusqlite::Connection::open(dir.path().join(FILE_NAME)).unwrap();
    conn.execute("UPDATE meta SET value = ?2 WHERE key = ?1", [key, value])
        .unwrap();
}

#[test]
fn a_missing_file_needs_a_rebuild_and_a_finished_one_does_not() {
    let dir = TestDir::new("open-missing");
    let (mut index, state) = SearchIndex::open(dir.path()).unwrap();
    assert!(state.needs_rebuild);
    answer(&mut index, 1, 1, 1, "kept across opens");
    index.finish_rebuild().unwrap();
    drop(index);
    let (index, state) = SearchIndex::open(dir.path()).unwrap();
    assert!(!state.needs_rebuild);
    assert_eq!(find(&index, "kept ", Mode::Ranked).hits.len(), 1);
}

#[test]
fn an_unfinished_build_needs_a_rebuild_and_is_emptied() {
    let dir = TestDir::new("open-unfinished");
    let (mut index, _) = SearchIndex::open(dir.path()).unwrap();
    answer(&mut index, 1, 1, 1, "half built");
    drop(index);
    let (index, state) = SearchIndex::open(dir.path()).unwrap();
    assert!(state.needs_rebuild);
    assert_eq!(count(&index, "half ", Mode::Ranked), 0);
}

#[test]
fn a_rebuild_that_never_finishes_reopens_as_needing_one() {
    let dir = TestDir::new("open-crashed-rebuild");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "from the first build");
    index.start_rebuild(10).unwrap();
    answer(&mut index, 1, 2, 1, "half of the second build");
    // Dropped before finish_rebuild, as a crash would leave it.
    drop(index);
    let (index, state) = SearchIndex::open(dir.path()).unwrap();
    assert!(state.needs_rebuild);
    assert_eq!(count(&index, "build ", Mode::Ranked), 0);
}

#[test]
fn another_format_or_tokenizer_needs_a_rebuild_and_is_emptied() {
    for (key, value) in [("format", "0"), ("tokenizer", "unicode61")] {
        let dir = TestDir::new(&format!("open-{key}"));
        let mut index = index(&dir);
        answer(&mut index, 1, 1, 1, "old format text");
        drop(index);
        set_meta(&dir, key, value);
        let (index, state) = SearchIndex::open(dir.path()).unwrap();
        assert!(state.needs_rebuild, "{key}");
        assert!(find(&index, "old ", Mode::Ranked).hits.is_empty(), "{key}");
        assert!(find(&index, "old", Mode::Typing).hits.is_empty(), "{key}");
    }
}

#[test]
fn an_index_of_the_previous_format_needs_a_rebuild() {
    // Format 1 was the index before the text was normalised: its rows are not searched again.
    let dir = TestDir::new("open-previous-format");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "stored before normalisation");
    drop(index);
    set_meta(&dir, "format", "1");
    let (index, state) = SearchIndex::open(dir.path()).unwrap();
    assert!(state.needs_rebuild);
    assert_eq!(count(&index, "stored ", Mode::Ranked), 0);
}

#[test]
fn a_file_that_is_not_a_database_needs_a_rebuild() {
    let dir = TestDir::new("open-garbage");
    write::write(&dir.path().join(FILE_NAME), &[0x5a; 8192]);
    let (mut index, state) = SearchIndex::open(dir.path()).unwrap();
    assert!(state.needs_rebuild);
    answer(&mut index, 1, 1, 1, "works after");
    assert_eq!(find(&index, "works ", Mode::Ranked).hits.len(), 1);
}

#[cfg(unix)]
#[test]
fn the_file_is_private_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TestDir::new("open-mode");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "private text");
    for name in [FILE_NAME.to_owned(), format!("{FILE_NAME}-wal")] {
        let mode = std::fs::metadata(dir.path().join(&name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{name}");
    }
}

#[test]
fn a_rebuild_reports_conversations_done_of_total() {
    let dir = TestDir::new("open-progress");
    let (mut index, _) = SearchIndex::open(dir.path()).unwrap();
    assert_eq!(index.progress(), None);
    index.start_rebuild(3).unwrap();
    for t in 1..=2 {
        index
            .batch(|batch| {
                batch.put_entry(
                    crate::common::matter(1),
                    crate::common::thread(t),
                    crate::common::seq(1),
                    clauseo_search::EntryKind::Answer,
                    crate::common::at(1),
                    "rebuilt text",
                )?;
                batch.conversation_done();
                Ok(())
            })
            .unwrap();
    }
    // A batch that fails counts nothing.
    let failed = index.batch(|batch| {
        batch.conversation_done();
        Err::<(), _>(clauseo_search::Error::Interrupted)
    });
    assert!(failed.is_err());
    let progress = index.progress().unwrap();
    assert_eq!((progress.done, progress.total), (2, 3));
    let page = find(&index, "rebuilt ", Mode::Ranked);
    assert_eq!(page.hits.len(), 2);
    assert_eq!(page.indexing, Some(progress));
    index.finish_rebuild().unwrap();
    assert_eq!(index.progress(), None);
    assert_eq!(find(&index, "rebuilt ", Mode::Ranked).indexing, None);
}

#[test]
fn watermarks_land_with_their_rows_and_a_failed_batch_keeps_neither() {
    use clauseo_protocol::StreamId;

    use crate::common::{seq, thread};
    let dir = TestDir::new("open-watermarks");
    let mut index = index(&dir);
    let stream = StreamId::Thread(thread(1));
    index
        .batch(|b| {
            b.set_watermark(stream, Some(seq(7)))?;
            Ok(())
        })
        .unwrap();
    let failed: Result<(), clauseo_search::Error> = index.batch(|b| {
        b.set_watermark(stream, Some(seq(9)))?;
        Err(clauseo_search::Error::Corrupt("stop".into()))
    });
    assert!(failed.is_err());
    assert_eq!(index.watermarks().unwrap(), vec![(stream, seq(7))]);
    drop(index);
    let (index, state) = SearchIndex::open(dir.path()).unwrap();
    assert!(!state.needs_rebuild);
    assert_eq!(index.watermarks().unwrap(), vec![(stream, seq(7))]);
    let mut index = index;
    index.batch(|b| b.set_watermark(stream, None)).unwrap();
    assert_eq!(index.watermarks().unwrap().len(), 0);
}

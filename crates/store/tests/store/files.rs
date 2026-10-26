//! The file itself: migrations, pragmas, the lock, the mode, torn and damaged files, a full
//! disk, and rows that do not read back.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;
use clauseo_store::{BlobIn, Commit, DB_FILE, Error, SCHEMA, Store, StoreOptions};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, seqs, text_event, texts, thread};
use crate::write;

fn db(dir: &Path) -> PathBuf {
    dir.join(DB_FILE)
}

fn wal(dir: &Path) -> PathBuf {
    dir.join(format!("{DB_FILE}-wal"))
}

fn open_at(dir: &Path) -> Result<Store, Error> {
    block_on(Store::open(StoreOptions::new(dir)))
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

/// Decision 0006 quotes schema version 1 in full; the quote must be the migration file, byte for
/// byte, so the two never drift.
#[test]
fn decision_record_quotes_the_migration() {
    let record = include_str!("../../../../docs/decisions/0006-store-format.md");
    let migration = include_str!("../../src/migrations/0001_init.sql");
    let start = record.find("```sql\n").expect("0006 has an sql block") + "```sql\n".len();
    let len = record[start..].find("```\n").expect("the sql block ends");
    assert_eq!(&record[start..start + len], migration);
}

#[test]
fn migrations_set_user_version() {
    let dir = TestDir::new("migrations_set_user_version");
    let store = dir.open();
    block_on(store.commit(texts(thread(1), 1, 2))).unwrap();
    block_on(store.close()).unwrap();

    let conn = Connection::open(db(dir.path())).unwrap();
    assert_eq!(user_version(&conn), i64::from(SCHEMA));
    let mut statement = conn
        .prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
        .unwrap();
    let found: BTreeSet<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop(statement);
    drop(conn);
    let expected: BTreeSet<(String, String)> = [
        ("table", "streams"),
        ("table", "events"),
        ("view", "events_readable"),
        ("table", "snapshots"),
        ("table", "blobs"),
        ("table", "receipts"),
        ("table", "raw_frames"),
        ("index", "raw_frames_ts"),
        ("table", "matters"),
        ("table", "threads"),
        ("index", "threads_by_matter"),
        ("table", "checkpoints"),
        ("index", "checkpoints_by_matter"),
        ("table", "account_limits"),
    ]
    .into_iter()
    .map(|(kind, name)| (kind.to_owned(), name.to_owned()))
    .collect();
    assert_eq!(found, expected);

    // A second open of a migrated file runs no migration again: the rows are still there.
    let store = dir.open();
    let read = block_on(store.read_after(thread(1), None, 10)).unwrap();
    assert_eq!(seqs(&read), vec![1, 2]);
    block_on(store.close()).unwrap();
}

#[test]
fn newer_user_version_is_refused() {
    let dir = TestDir::new("newer_user_version_is_refused");
    block_on(dir.open().close()).unwrap();
    let conn = Connection::open(db(dir.path())).unwrap();
    conn.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA + 1))
        .unwrap();
    drop(conn);

    let error = open_at(dir.path()).err().unwrap();
    let newer = SCHEMA + 1;
    assert!(
        matches!(error, Error::NewerSchema { found, supported }
            if found == newer && supported == SCHEMA),
        "{error:?}"
    );
    assert_eq!(error.code(), ErrorCode::StoreNewerVersion);

    // The refused open changed nothing: the file still says the newer version.
    let conn = Connection::open(db(dir.path())).unwrap();
    assert_eq!(user_version(&conn), i64::from(SCHEMA + 1));
}

#[test]
fn pragmas_are_set() {
    let dir = TestDir::new("pragmas_are_set");
    let store = dir.open();
    let writer = |name| block_on(store.writer_pragma(name)).unwrap();
    let reader = |name| block_on(store.reader_pragma(name)).unwrap();
    assert_eq!(writer("page_size"), "16384");
    assert_eq!(writer("journal_mode"), "wal");
    assert_eq!(writer("synchronous"), "2");
    assert_eq!(writer("temp_store"), "2");
    assert_eq!(writer("foreign_keys"), "1");
    assert_eq!(writer("busy_timeout"), "5000");
    assert_eq!(writer("journal_size_limit"), "33554432");
    assert_eq!(writer("query_only"), "0");
    #[cfg(target_os = "macos")]
    assert_eq!(writer("fullfsync"), "1");
    #[cfg(not(target_os = "macos"))]
    assert_eq!(writer("fullfsync"), "0");

    assert_eq!(reader("query_only"), "1");
    assert_eq!(reader("mmap_size"), "0");
    assert_eq!(reader("cache_size"), "-2000");
    assert_eq!(reader("busy_timeout"), "5000");
    assert_eq!(reader("page_size"), "16384");
    assert_eq!(reader("journal_mode"), "wal");
    block_on(store.close()).unwrap();
}

#[test]
fn second_open_is_locked() {
    let dir = TestDir::new("second_open_is_locked");
    let store = dir.open();
    let error = open_at(dir.path()).err().unwrap();
    assert!(matches!(error, Error::Locked), "{error:?}");
    assert_eq!(error.code(), ErrorCode::StoreLocked);
    // The refused open did not disturb the first.
    block_on(store.commit(texts(thread(1), 1, 1))).unwrap();
    block_on(store.close()).unwrap();

    let store = open_at(dir.path()).unwrap();
    let read = block_on(store.read_after(thread(1), None, 10)).unwrap();
    assert_eq!(seqs(&read), vec![1]);
    block_on(store.close()).unwrap();
}

/// Copies `state.sqlite` and its WAL from `from` into `to`.
fn copy_db(from: &Path, to: &Path) {
    write::copy(&db(from), &db(to));
    write::copy(&wal(from), &wal(to));
}

#[test]
fn torn_wal_tail_drops_last_commit() {
    let dir = TestDir::new("torn_wal_tail_drops_last_commit");
    let store = dir.open();
    block_on(store.commit(texts(thread(1), 1, 1))).unwrap();
    let wal_after_a = std::fs::metadata(wal(dir.path())).unwrap().len();
    block_on(store.commit(texts(thread(1), 2, 2))).unwrap();
    let wal_after_b = std::fs::metadata(wal(dir.path())).unwrap().len();
    assert!(wal_after_b > wal_after_a, "B appended frames to the WAL");
    // Copied while open: close would checkpoint the WAL into the file and truncate it.
    let whole = dir.sub("whole");
    let torn = dir.sub("torn");
    copy_db(dir.path(), &whole);
    copy_db(dir.path(), &torn);
    block_on(store.close()).unwrap();

    // The uncut copy holds both commits.
    let store = open_at(&whole).unwrap();
    let read = block_on(store.read_after(thread(1), None, 10)).unwrap();
    assert_eq!(seqs(&read), vec![1, 2]);
    block_on(store.close()).unwrap();

    // Cut inside the WAL's last frame, which is B's commit frame.
    write::truncate(&wal(&torn), wal_after_b - 100);
    let store = open_at(&torn).unwrap();
    let read = block_on(store.read_after(thread(1), None, 10)).unwrap();
    assert_eq!(seqs(&read), vec![1]);
    let heads = block_on(store.heads()).unwrap();
    assert_eq!(heads, vec![(thread(1), crate::common::seq(1))]);
    // B's seq is free again.
    block_on(store.commit(texts(thread(1), 2, 2))).unwrap();
    let read = block_on(store.read_after(thread(1), None, 10)).unwrap();
    assert_eq!(seqs(&read), vec![1, 2]);
    block_on(store.close()).unwrap();
}

/// `len` bytes from a fixed xorshift sequence: no SQLite header, no structure.
fn noise(len: usize) -> Vec<u8> {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

#[test]
fn garbage_file_is_corrupt() {
    let dir = TestDir::new("garbage_file_is_corrupt");
    write::write(&db(dir.path()), &noise(64 * 1024));
    let error = open_at(dir.path()).err().unwrap();
    assert!(matches!(error, Error::Corrupt { .. }), "{error:?}");
    assert_eq!(error.code(), ErrorCode::StoreCorrupt);
}

#[test]
fn truncated_file_is_corrupt() {
    let dir = TestDir::new("truncated_file_is_corrupt");
    let store = dir.open();
    for n in 1..=40 {
        let event = text_event(thread(1), n, &"x".repeat(4000));
        let commit = Commit {
            events: vec![event],
            ..Commit::default()
        };
        block_on(store.commit(commit)).unwrap();
    }
    block_on(store.close()).unwrap();
    let len = std::fs::metadata(db(dir.path())).unwrap().len();
    assert!(len >= 10 * 16384, "the file spans many pages: {len} bytes");
    write::truncate(&db(dir.path()), len / 2);

    // Page 1 still says the file holds every page; SQLite refuses the shorter file on open.
    let header = std::fs::read(db(dir.path())).unwrap();
    let pages = u32::from_be_bytes(header[28..32].try_into().unwrap());
    assert_eq!(u64::from(pages) * 16384, len, "the header's page count");
    let error = open_at(dir.path()).err().unwrap();
    assert!(matches!(error, Error::Corrupt { .. }), "{error:?}");
    assert_eq!(error.code(), ErrorCode::StoreCorrupt);
}

#[test]
fn disk_full_maps_to_store_disk_full() {
    let dir = TestDir::new("disk_full_maps_to_store_disk_full");
    let options = StoreOptions {
        max_page_count: Some(40),
        ..StoreOptions::new(dir.path())
    };
    let store = block_on(Store::open(options)).unwrap();
    let big = BlobIn::new(noise(2 * 1024 * 1024));
    let hash = big.hash();
    let error = block_on(store.commit(Commit {
        events: vec![text_event(thread(1), 1, "names the big blob")],
        blobs: vec![big],
        ..Commit::default()
    }))
    .unwrap_err();
    assert!(matches!(error, Error::DiskFull), "{error:?}");
    assert_eq!(error.code(), ErrorCode::StoreDiskFull);

    // Nothing of the failed commit is there.
    assert_eq!(block_on(store.heads()).unwrap(), Vec::new());
    let read = block_on(store.read_after(thread(1), None, 10)).unwrap();
    assert_eq!(seqs(&read), Vec::<u64>::new());
    let missing = block_on(store.read_blob(hash, 0, 16)).unwrap_err();
    assert!(matches!(missing, Error::BlobMissing { .. }), "{missing:?}");

    // A small commit still fits, at the seq the failed one would have taken.
    let small = BlobIn::new(b"small".to_vec());
    let small_hash = small.hash();
    block_on(store.commit(Commit {
        events: vec![text_event(thread(1), 1, "small")],
        blobs: vec![small],
        ..Commit::default()
    }))
    .unwrap();
    let read = block_on(store.read_after(thread(1), None, 10)).unwrap();
    assert_eq!(seqs(&read), vec![1]);
    assert_eq!(
        block_on(store.read_blob(small_hash, 0, 16)).unwrap(),
        b"small"
    );
    block_on(store.close()).unwrap();
}

#[cfg(unix)]
#[test]
fn db_file_is_private() {
    use std::os::unix::fs::PermissionsExt;

    use clauseo_store::LOCK_FILE;

    let dir = TestDir::new("db_file_is_private");
    let store = dir.open();
    block_on(store.commit(texts(thread(1), 1, 1))).unwrap();
    let mode = |name: &str| {
        let metadata = std::fs::metadata(dir.path().join(name)).unwrap();
        metadata.permissions().mode() & 0o777
    };
    assert_eq!(mode(DB_FILE), 0o600, "{DB_FILE}");
    assert_eq!(mode(&format!("{DB_FILE}-wal")), 0o600, "{DB_FILE}-wal");
    assert_eq!(mode(&format!("{DB_FILE}-shm")), 0o600, "{DB_FILE}-shm");
    assert_eq!(mode(LOCK_FILE), 0o600, "{LOCK_FILE}");
    block_on(store.close()).unwrap();
}

/// A closed store holding seqs 1 to 3 of thread 1, opened with rusqlite for patching.
fn three_rows(dir: &TestDir) -> Connection {
    let store = dir.open();
    block_on(store.commit(texts(thread(1), 1, 3))).unwrap();
    block_on(store.close()).unwrap();
    Connection::open(db(dir.path())).unwrap()
}

/// The `events` key of `seq` in the first stream the store numbered.
fn key(seq: i64) -> i64 {
    (1 << 32) | seq
}

/// Reads thread 1 from the start in a fresh store over `dir`, and closes it.
fn read_thread_1(dir: &TestDir) -> Result<Vec<u64>, Error> {
    let store = dir.open();
    let read = block_on(store.read_after(thread(1), None, 10)).map(|events| seqs(&events));
    block_on(store.close()).unwrap();
    read
}

#[test]
fn row_naming_another_place_is_corrupt() {
    let dir = TestDir::new("row_naming_another_place_is_corrupt");
    let conn = three_rows(&dir);
    // Row 1 gets row 2's envelope, which names seq 2.
    conn.execute(
        "UPDATE events SET envelope = (SELECT envelope FROM events WHERE key = ?2) WHERE key = ?1",
        [key(1), key(2)],
    )
    .unwrap();
    drop(conn);
    let error = read_thread_1(&dir).unwrap_err();
    assert!(matches!(error, Error::Corrupt { .. }), "{error:?}");
    assert_eq!(error.code(), ErrorCode::StoreCorrupt);
}

#[test]
fn row_naming_another_stream_is_corrupt() {
    let dir = TestDir::new("row_naming_another_stream_is_corrupt");
    let store = dir.open();
    block_on(store.commit(texts(thread(1), 1, 1))).unwrap();
    block_on(store.commit(texts(thread(2), 1, 1))).unwrap();
    block_on(store.close()).unwrap();
    let conn = Connection::open(db(dir.path())).unwrap();
    // Thread 1's seq 1 gets thread 2's seq 1 envelope (stream 2 is numbered 2).
    conn.execute(
        "UPDATE events SET envelope = (SELECT envelope FROM events WHERE key = ?2) WHERE key = ?1",
        [key(1), (2 << 32) | 1],
    )
    .unwrap();
    drop(conn);
    let error = read_thread_1(&dir).unwrap_err();
    assert!(matches!(error, Error::Corrupt { .. }), "{error:?}");
}

#[test]
fn garbage_envelope_is_corrupt() {
    let dir = TestDir::new("garbage_envelope_is_corrupt");
    let conn = three_rows(&dir);
    conn.execute(
        "UPDATE events SET envelope = ?2 WHERE key = ?1",
        rusqlite::params![key(2), noise(200)],
    )
    .unwrap();
    drop(conn);
    let error = read_thread_1(&dir).unwrap_err();
    assert!(matches!(error, Error::Corrupt { .. }), "{error:?}");
    assert_eq!(error.code(), ErrorCode::StoreCorrupt);
}

#[test]
fn seq_gap_on_read_is_corrupt() {
    let dir = TestDir::new("seq_gap_on_read_is_corrupt");
    let conn = three_rows(&dir);
    assert_eq!(
        conn.execute("DELETE FROM events WHERE key = ?1", [key(2)])
            .unwrap(),
        1
    );
    drop(conn);
    let error = read_thread_1(&dir).unwrap_err();
    assert!(matches!(error, Error::Corrupt { .. }), "{error:?}");
    assert_eq!(error.code(), ErrorCode::StoreCorrupt);
    // Reading after the gap finds no gap within what it reads.
    let store = dir.open();
    let tail = block_on(store.read_after(thread(1), Some(crate::common::seq(2)), 10)).unwrap();
    assert_eq!(seqs(&tail), vec![3]);
    block_on(store.close()).unwrap();
}

//! `Error::code` and `Error::retryable` for every variant, from real failures where the
//! store can be made to fail that way.

use clauseo_protocol::{ErrorCode, MatterKind};
use clauseo_store::{BlobIn, Commit, DB_FILE, Error, MatterRow, SCHEMA, Store, StoreOptions};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, matter, receipt, text_event, texts, thread, ts};
use crate::write;

/// The code and retryable flag each variant must give. Exhaustive, so a new variant fails to
/// compile here until it has a row. `retryable` follows `ErrorCode::is_retryable`, except
/// `store.io`: retryable only for `SQLITE_BUSY` and `Closed`.
fn expected(error: &Error) -> (ErrorCode, bool) {
    match error {
        Error::DiskFull => (ErrorCode::StoreDiskFull, false),
        Error::Corrupt { .. } | Error::BlobMissing { .. } => (ErrorCode::StoreCorrupt, false),
        Error::Io { source } => (ErrorCode::StoreIo, source.is_busy()),
        Error::Closed => (ErrorCode::StoreIo, true),
        Error::Locked => (ErrorCode::StoreLocked, false),
        Error::NewerSchema { .. } => (ErrorCode::StoreNewerVersion, false),
        Error::SeqGap { .. }
        | Error::DuplicateCommand { .. }
        | Error::KeyOutOfRange { .. }
        | Error::Encode { .. } => (ErrorCode::Internal, false),
    }
}

fn check(error: &Error, code: ErrorCode, retryable: bool) {
    assert_eq!(
        expected(error),
        (code, retryable),
        "table row for {error:?}"
    );
    assert_eq!(error.code(), code, "code of {error:?}");
    assert_eq!(error.retryable(), retryable, "retryable of {error:?}");
    if !matches!(error, Error::Io { .. } | Error::Closed) {
        assert_eq!(
            error.retryable(),
            code.is_retryable(),
            "{error:?} follows protocol"
        );
    }
}

#[test]
fn error_codes_map() {
    let dir = TestDir::new("error_codes_map");

    // Io: a folder that does not exist; an OS error, not SQLITE_BUSY.
    let io = block_on(Store::open(StoreOptions::new(dir.path().join("missing")))).err();
    let io = io.unwrap();
    assert!(
        matches!(&io, Error::Io { source } if !source.is_busy()),
        "{io:?}"
    );
    check(&io, ErrorCode::StoreIo, false);
    // Io from SQLite: a constraint failure (a thread whose matter has no row).
    let store = dir.open();
    let row = clauseo_store::ThreadRow {
        id: crate::common::id(1),
        matter: matter(9),
        agent: clauseo_protocol::AgentId::new("claude-code").unwrap(),
        title: None,
        acp_session_id: None,
        archived: false,
        created: ts(0),
        updated: ts(0),
    };
    let constraint = block_on(store.commit(Commit {
        thread: Some(row),
        ..Commit::default()
    }))
    .unwrap_err();
    assert!(matches!(constraint, Error::Io { .. }), "{constraint:?}");
    check(&constraint, ErrorCode::StoreIo, false);

    // Locked.
    let locked = block_on(Store::open(StoreOptions::new(dir.path())))
        .err()
        .unwrap();
    assert!(matches!(locked, Error::Locked), "{locked:?}");
    check(&locked, ErrorCode::StoreLocked, false);

    // SeqGap.
    let gap = block_on(store.commit(texts(thread(1), 2, 2))).unwrap_err();
    assert!(matches!(gap, Error::SeqGap { .. }), "{gap:?}");
    check(&gap, ErrorCode::Internal, false);

    // DuplicateCommand.
    let with_receipt = || Commit {
        receipt: Some(receipt(1, None)),
        ..Commit::default()
    };
    block_on(store.commit(with_receipt())).unwrap();
    let duplicate = block_on(store.commit(with_receipt())).unwrap_err();
    assert!(
        matches!(duplicate, Error::DuplicateCommand { .. }),
        "{duplicate:?}"
    );
    check(&duplicate, ErrorCode::Internal, false);

    // Encode: a matter of unknown kind has no stored form.
    let unknown = MatterRow {
        id: matter(1),
        name: "m".to_owned(),
        kind: MatterKind::Unknown,
        folder_path: "/m".to_owned(),
        folder_locator: None,
        shadow_repo_path: "/s".to_owned(),
        created: ts(0),
    };
    let encode = block_on(store.commit(Commit {
        matter: Some(unknown),
        ..Commit::default()
    }))
    .unwrap_err();
    assert!(matches!(encode, Error::Encode { .. }), "{encode:?}");
    check(&encode, ErrorCode::Internal, false);

    // BlobMissing.
    let hash = BlobIn::new(b"never stored".to_vec()).hash();
    let missing = block_on(store.read_blob(hash, 0, 8)).unwrap_err();
    assert!(matches!(missing, Error::BlobMissing { .. }), "{missing:?}");
    check(&missing, ErrorCode::StoreCorrupt, false);

    // KeyOutOfRange needs a seq at 2^32 or a 2^31st stream; built directly.
    let key = Error::KeyOutOfRange { stream: thread(1) };
    check(&key, ErrorCode::Internal, false);

    // Closed: a clone's call after close, read and write side.
    let clone = store.clone();
    block_on(store.close()).unwrap();
    let closed = block_on(clone.heads()).unwrap_err();
    assert!(matches!(closed, Error::Closed), "{closed:?}");
    check(&closed, ErrorCode::StoreIo, true);
    let closed = block_on(clone.read_after(thread(1), None, 1)).unwrap_err();
    assert!(matches!(closed, Error::Closed), "{closed:?}");
    drop(clone);

    // NewerSchema.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA + 1))
        .unwrap();
    drop(conn);
    let newer = block_on(Store::open(StoreOptions::new(dir.path())))
        .err()
        .unwrap();
    assert!(matches!(newer, Error::NewerSchema { .. }), "{newer:?}");
    check(&newer, ErrorCode::StoreNewerVersion, false);
}

#[test]
fn damaged_files_map_to_corrupt_and_disk_full() {
    let dir = TestDir::new("damaged_files_map_to_corrupt_and_disk_full");

    // DiskFull: SQLITE_FULL from a page cap.
    let full = dir.sub("full");
    let options = StoreOptions {
        max_page_count: Some(40),
        ..StoreOptions::new(&full)
    };
    let store = block_on(Store::open(options)).unwrap();
    let big = BlobIn::new((0..2_000_000_u32).map(|n| (n * 7 % 251) as u8).collect());
    let disk_full = block_on(store.commit(Commit {
        events: vec![text_event(thread(1), 1, "big")],
        blobs: vec![big],
        ..Commit::default()
    }))
    .unwrap_err();
    assert!(matches!(disk_full, Error::DiskFull), "{disk_full:?}");
    check(&disk_full, ErrorCode::StoreDiskFull, false);
    block_on(store.close()).unwrap();

    // Corrupt: SQLITE_NOTADB from a file that is not a database.
    let garbage = dir.sub("garbage");
    write::write(&garbage.join(DB_FILE), &vec![0x5A; 32 * 1024]);
    let corrupt = block_on(Store::open(StoreOptions::new(&garbage)))
        .err()
        .unwrap();
    assert!(matches!(corrupt, Error::Corrupt { .. }), "{corrupt:?}");
    check(&corrupt, ErrorCode::StoreCorrupt, false);
}

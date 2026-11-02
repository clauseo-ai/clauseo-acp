//! A matter's details (schema 2): a database from schema 1 opens and lists its matters with the
//! defaults, the details round-trip and a value that does not parse is corrupt, and the time of
//! each stream's last event reads without the log.

use clauseo_protocol::{ClientName, Date, ErrorCode, MatterKind, StreamId};
use clauseo_store::{Commit, DB_FILE, Error, MatterRow, SCHEMA};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, matter, texts, thread, ts};

/// The matter row of `n`, with no details.
fn plain_row(n: u64) -> MatterRow {
    MatterRow {
        id: matter(n),
        name: format!("Matter {n}"),
        kind: MatterKind::Folder,
        folder_path: format!("/matters/{n}"),
        folder_locator: None,
        shadow_repo_path: format!("/shadow/{n}"),
        created: ts(i64::try_from(n).unwrap()),
        client: None,
        next_date: None,
        pinned: false,
        last_opened: None,
        forgotten: None,
    }
}

#[test]
fn a_schema_1_database_opens_and_lists_its_matters_with_the_defaults() {
    let dir = TestDir::new("schema_1_matters");
    // A file as a schema 1 build left it: the first migration alone, one matter in it.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute_batch(&format!(
        "BEGIN;\n{}\nPRAGMA user_version = 1;\nCOMMIT;",
        include_str!("../../src/migrations/0001_init.sql")
    ))
    .unwrap();
    let old = plain_row(1);
    conn.execute(
        "INSERT INTO matters (id, name, kind, folder_path, folder_locator, shadow_repo_path, \
         created_ms) VALUES (?1, ?2, 'folder', ?3, NULL, ?4, ?5)",
        rusqlite::params![
            old.id.to_string(),
            old.name,
            old.folder_path,
            old.shadow_repo_path,
            old.created.as_millis(),
        ],
    )
    .unwrap();
    drop(conn);

    let store = dir.open();
    assert_eq!(block_on(store.matters()).unwrap(), vec![old.clone()]);
    // The migrated row takes details like a new one.
    let detailed = MatterRow {
        client: Some(ClientName::new("Rathi Steels").unwrap()),
        pinned: true,
        ..old
    };
    block_on(store.commit(Commit {
        matter: Some(detailed.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, i64::from(SCHEMA));
    assert_eq!(SCHEMA, 2);
    drop(conn);
    let store = dir.open();
    assert_eq!(block_on(store.matters()).unwrap(), vec![detailed]);
    block_on(store.close()).unwrap();
}

#[test]
fn details_round_trip_and_a_second_put_replaces_them() {
    let dir = TestDir::new("matter_details_round_trip");
    let store = dir.open();
    let full = MatterRow {
        client: Some(ClientName::new("State of Maharashtra").unwrap()),
        next_date: Some("2026-11-20".parse::<Date>().unwrap()),
        pinned: true,
        last_opened: Some(ts(40)),
        forgotten: Some(ts(50)),
        ..plain_row(1)
    };
    block_on(store.commit(Commit {
        matter: Some(full.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let store = dir.open();
    assert_eq!(block_on(store.matters()).unwrap(), vec![full.clone()]);
    let cleared = MatterRow {
        client: None,
        next_date: None,
        pinned: false,
        forgotten: None,
        ..full
    };
    block_on(store.commit(Commit {
        matter: Some(cleared.clone()),
        ..Commit::default()
    }))
    .unwrap();
    assert_eq!(block_on(store.matters()).unwrap(), vec![cleared]);
    block_on(store.close()).unwrap();
}

#[test]
fn a_stored_client_or_date_that_does_not_parse_is_corrupt() {
    for (column, value) in [("client", "   "), ("next_date", "2026-02-30")] {
        let dir = TestDir::new(&format!("matter_detail_corrupt_{column}"));
        let store = dir.open();
        block_on(store.commit(Commit {
            matter: Some(plain_row(1)),
            ..Commit::default()
        }))
        .unwrap();
        block_on(store.close()).unwrap();
        let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
        conn.execute(&format!("UPDATE matters SET {column} = ?1"), [value])
            .unwrap();
        drop(conn);
        let store = dir.open();
        let error = block_on(store.matters()).unwrap_err();
        assert!(
            matches!(error, Error::Corrupt { .. }),
            "{column}: {error:?}"
        );
        assert_eq!(error.code(), ErrorCode::StoreCorrupt);
        block_on(store.close()).unwrap();
    }
}

#[test]
fn tail_times_give_each_streams_last_event_time() {
    let dir = TestDir::new("tail_times");
    let store = dir.open();
    assert_eq!(block_on(store.tail_times()).unwrap(), Vec::new());
    block_on(store.commit(texts(thread(1), 1, 3))).unwrap();
    block_on(store.commit(texts(thread(2), 1, 1))).unwrap();
    block_on(store.commit(texts(thread(1), 4, 7))).unwrap();
    let mut tails = block_on(store.tail_times()).unwrap();
    tails.sort_unstable();
    let mut expected: Vec<(StreamId, _)> = vec![(thread(1), ts(7)), (thread(2), ts(1))];
    expected.sort_unstable();
    assert_eq!(tails, expected);
    block_on(store.close()).unwrap();
}

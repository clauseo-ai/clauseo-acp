//! A thread's seen mark (schema 3): a database from schema 2 opens with every existing thread
//! seen up to its stream's last `seq` (a thread with no events: never seen), the mark
//! round-trips and clears, and a stored value that is no `seq` is corrupt.

use clauseo_protocol::{AgentId, ErrorCode, MatterId, MatterKind, ThreadId};
use clauseo_store::{Commit, DB_FILE, Error, MatterRow, SCHEMA, ThreadRow};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, id, matter, seq, texts, ts};

fn matter_row(n: u64) -> MatterRow {
    MatterRow {
        id: matter(n),
        name: format!("Matter {n}"),
        kind: MatterKind::Folder,
        folder_path: format!("/matters/{n}"),
        folder_locator: None,
        shadow_repo_path: format!("/shadow/{n}"),
        created: ts(0),
        client: None,
        next_date: None,
        pinned: false,
        last_opened: None,
        forgotten: None,
    }
}

fn thread_row(n: u64, matter: MatterId) -> ThreadRow {
    ThreadRow {
        id: id::<ThreadId>(n),
        matter,
        agent: AgentId::new("claude-code").unwrap(),
        title: None,
        acp_session_id: None,
        archived: false,
        created: ts(1),
        updated: ts(i64::try_from(n).unwrap()),
        seen_seq: None,
    }
}

#[test]
fn a_schema_2_database_opens_with_its_threads_seen_up_to_their_last_seq() {
    let dir = TestDir::new("schema_2_seen");
    // A file as a schema 2 build left it: the first two migrations, a matter, a thread with
    // five events and one with none.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute_batch(&format!(
        "BEGIN;\n{}\n{}\nPRAGMA user_version = 2;\nCOMMIT;",
        include_str!("../../src/migrations/0001_init.sql"),
        include_str!("../../src/migrations/0002_matter_details.sql"),
    ))
    .unwrap();
    let with_events = id::<ThreadId>(1);
    let empty = id::<ThreadId>(2);
    conn.execute_batch(&format!(
        "INSERT INTO matters (id, name, kind, folder_path, shadow_repo_path, created_ms) \
         VALUES ('{m}', 'Matter 1', 'folder', '/matters/1', '/shadow/1', 0);
         INSERT INTO threads (id, matter_id, agent_id, created_ms, updated_ms) \
         VALUES ('{with_events}', '{m}', 'claude-code', 1, 1), ('{empty}', '{m}', 'claude-code', 1, 2);
         INSERT INTO streams (stream_id, stream, last_seq) VALUES (1, 'thread:{with_events}', 5);",
        m = matter(1),
    ))
    .unwrap();
    drop(conn);

    let store = dir.open();
    let seen: Vec<_> = block_on(store.threads(matter(1)))
        .unwrap()
        .into_iter()
        .map(|row| (row.id, row.seen_seq))
        .collect();
    assert_eq!(seen, [(empty, None), (with_events, Some(seq(5)))]);
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, i64::from(SCHEMA));
    assert_eq!(SCHEMA, 3);
}

#[test]
fn the_seen_mark_round_trips_and_clears() {
    let dir = TestDir::new("seen_round_trip");
    let store = dir.open();
    block_on(store.commit(Commit {
        matter: Some(matter_row(1)),
        ..Commit::default()
    }))
    .unwrap();
    let marked = ThreadRow {
        seen_seq: Some(seq(7)),
        ..thread_row(1, matter(1))
    };
    block_on(store.commit(Commit {
        thread: Some(marked.clone()),
        ..texts(crate::common::thread(1), 1, 7)
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let store = dir.open();
    assert_eq!(
        block_on(store.threads(matter(1))).unwrap(),
        std::slice::from_ref(&marked)
    );
    let cleared = ThreadRow {
        seen_seq: None,
        ..marked
    };
    block_on(store.commit(Commit {
        thread: Some(cleared.clone()),
        ..Commit::default()
    }))
    .unwrap();
    assert_eq!(block_on(store.threads(matter(1))).unwrap(), [cleared]);
    block_on(store.close()).unwrap();
}

#[test]
fn a_stored_seen_mark_that_is_no_seq_is_corrupt() {
    for value in [0_i64, -3] {
        let dir = TestDir::new(&format!("seen_corrupt_{}", value.unsigned_abs()));
        let store = dir.open();
        block_on(store.commit(Commit {
            matter: Some(matter_row(1)),
            thread: Some(thread_row(1, matter(1))),
            ..Commit::default()
        }))
        .unwrap();
        block_on(store.close()).unwrap();
        let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
        conn.execute("UPDATE threads SET seen_seq = ?1", [value])
            .unwrap();
        drop(conn);
        let store = dir.open();
        let error = block_on(store.threads(matter(1))).unwrap_err();
        assert!(matches!(error, Error::Corrupt { .. }), "{value}: {error:?}");
        assert_eq!(error.code(), ErrorCode::StoreCorrupt);
        block_on(store.close()).unwrap();
    }
}

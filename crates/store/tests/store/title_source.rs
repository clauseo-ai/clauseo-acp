//! A thread's title source (schema 4): a database from schema 3 opens with its titles and no
//! source, each source round-trips through a reopen, and a source text this build does not
//! know reads as `Unknown`.

use clauseo_protocol::{AgentId, MatterId, MatterKind, RenameSource, ThreadId};
use clauseo_store::{Commit, DB_FILE, MatterRow, SCHEMA, ThreadRow};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, id, matter, ts};

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

fn thread_row(
    n: u64,
    matter: MatterId,
    title: Option<&str>,
    source: Option<RenameSource>,
) -> ThreadRow {
    ThreadRow {
        id: id::<ThreadId>(n),
        matter,
        agent: AgentId::new("claude-code").unwrap(),
        title: title.map(str::to_owned),
        title_source: source,
        acp_session_id: None,
        archived: false,
        created: ts(1),
        updated: ts(i64::try_from(n).unwrap()),
        seen_seq: None,
    }
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn a_schema_3_database_keeps_its_titles_with_no_source() {
    let dir = TestDir::new("schema_3_title_source");
    // A file as a schema 3 build left it: the first three migrations, a matter, and a thread
    // with a title it was given before schema 4 kept the source.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute_batch(&format!(
        "BEGIN;\n{}\n{}\n{}\nPRAGMA user_version = 3;\nCOMMIT;",
        include_str!("../../src/migrations/0001_init.sql"),
        include_str!("../../src/migrations/0002_matter_details.sql"),
        include_str!("../../src/migrations/0003_thread_seen.sql"),
    ))
    .unwrap();
    let old = id::<ThreadId>(1);
    conn.execute_batch(&format!(
        "INSERT INTO matters (id, name, kind, folder_path, shadow_repo_path, created_ms) \
         VALUES ('{m}', 'Matter 1', 'folder', '/matters/1', '/shadow/1', 0);
         INSERT INTO threads (id, matter_id, agent_id, title, created_ms, updated_ms) \
         VALUES ('{old}', '{m}', 'claude-code', 'Lease review', 1, 1);",
        m = matter(1),
    ))
    .unwrap();
    drop(conn);

    let store = dir.open();
    let rows = block_on(store.threads(matter(1))).unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| (row.title.as_deref(), row.title_source))
            .collect::<Vec<_>>(),
        [(Some("Lease review"), None)]
    );
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    assert_eq!(user_version(&conn), i64::from(SCHEMA));
    assert_eq!(SCHEMA, 4);
}

#[test]
fn each_title_source_round_trips_through_a_reopen() {
    let dir = TestDir::new("title_source_round_trip");
    let store = dir.open();
    block_on(store.commit(Commit {
        matter: Some(matter_row(1)),
        ..Commit::default()
    }))
    .unwrap();
    let rows = [
        thread_row(1, matter(1), Some("Typed"), Some(RenameSource::Lawyer)),
        thread_row(2, matter(1), Some("Agent"), Some(RenameSource::Agent)),
        thread_row(
            3,
            matter(1),
            Some("Stand-in"),
            Some(RenameSource::Placeholder),
        ),
        thread_row(4, matter(1), Some("Newer"), Some(RenameSource::Unknown)),
        thread_row(5, matter(1), None, None),
    ];
    for row in &rows {
        block_on(store.commit(Commit {
            thread: Some(row.clone()),
            ..Commit::default()
        }))
        .unwrap();
    }
    block_on(store.close()).unwrap();

    let store = dir.open();
    let mut stored = block_on(store.threads(matter(1))).unwrap();
    stored.sort_by_key(|row| row.id);
    assert_eq!(stored, rows);
    block_on(store.close()).unwrap();
}

#[test]
fn a_source_text_this_build_does_not_know_reads_as_unknown() {
    let dir = TestDir::new("title_source_unknown_text");
    let store = dir.open();
    block_on(store.commit(Commit {
        matter: Some(matter_row(1)),
        thread: Some(thread_row(
            1,
            matter(1),
            Some("Later"),
            Some(RenameSource::Agent),
        )),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    // A later build wrote a source this one has no name for.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute(
        "UPDATE threads SET title_source = 'from_a_newer_clauseo'",
        [],
    )
    .unwrap();
    drop(conn);
    let store = dir.open();
    let rows = block_on(store.threads(matter(1))).unwrap();
    assert_eq!(rows[0].title_source, Some(RenameSource::Unknown));
    block_on(store.close()).unwrap();
}

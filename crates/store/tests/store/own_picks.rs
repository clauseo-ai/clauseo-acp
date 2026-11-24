//! A thread's own model and effort (schema 9, `ThreadRow::own_picks`): a database from schema 7
//! opens with no own picks on its threads and takes them after, own picks round-trip through a
//! reopen, and stored own picks that do not parse are corrupt.

use clauseo_protocol::{
    AgentChoiceId, AgentId, ConfigPicks, ErrorCode, MatterId, MatterKind, ThreadId,
};
use clauseo_store::{Commit, DB_FILE, Error, MatterRow, SCHEMA, ThreadRow};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, id, matter, ts};

fn thread_row(n: u64, matter: MatterId, own_picks: Option<ConfigPicks>) -> ThreadRow {
    ThreadRow {
        id: id::<ThreadId>(n),
        matter,
        agent: AgentId::new("codex-acp").unwrap(),
        title: None,
        title_source: None,
        acp_session_id: None,
        archived: false,
        created: ts(1),
        updated: ts(i64::try_from(n).unwrap()),
        seen_seq: None,
        own_picks,
    }
}

fn picks(pairs: &[(&str, &str)]) -> ConfigPicks {
    let id = |text: &str| AgentChoiceId::new(text).unwrap();
    ConfigPicks::new(pairs.iter().map(|(o, v)| (id(o), id(v))).collect()).unwrap()
}

fn matter_commit() -> Commit {
    Commit {
        matter: Some(MatterRow {
            id: matter(1),
            name: "Matter 1".to_owned(),
            kind: MatterKind::Folder,
            folder_path: "/matters/1".to_owned(),
            folder_locator: None,
            shadow_repo_path: "/shadow/1".to_owned(),
            created: ts(0),
            client: None,
            next_date: None,
            pinned: false,
            last_opened: None,
            forgotten: None,
            skill_packs: None,
            agent_prefs: None,
            addon_switches: None,
        }),
        ..Commit::default()
    }
}

/// A matter and one thread whose own picks hold `model` = `gpt-6-luna`, in one commit.
pub(crate) fn kept_thread() -> Commit {
    Commit {
        thread: Some(thread_row(
            1,
            matter(1),
            Some(picks(&[("model", "gpt-6-luna")])),
        )),
        ..matter_commit()
    }
}

#[test]
fn own_picks_round_trip_through_a_reopen() {
    let dir = TestDir::new("own_picks_round_trip");
    let store = dir.open();
    block_on(store.commit(matter_commit())).unwrap();
    let luna = picks(&[("model", "gpt-6-luna"), ("reasoning_effort", "low")]);
    let rows = [
        thread_row(2, matter(1), Some(luna)),
        thread_row(1, matter(1), None),
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
    assert_eq!(block_on(store.threads(matter(1))).unwrap(), rows);
    block_on(store.close()).unwrap();
}

#[test]
fn stored_own_picks_that_do_not_parse_are_corrupt() {
    for (name, value) in [("not_json", "{"), ("not_a_value", r#"{"model":1}"#)] {
        let dir = TestDir::new(&format!("own_picks_corrupt_{name}"));
        let store = dir.open();
        block_on(store.commit(matter_commit())).unwrap();
        block_on(store.commit(Commit {
            thread: Some(thread_row(1, matter(1), None)),
            ..Commit::default()
        }))
        .unwrap();
        block_on(store.close()).unwrap();
        let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
        conn.execute("UPDATE threads SET own_picks = ?1", [value])
            .unwrap();
        drop(conn);
        let store = dir.open();
        let error = block_on(store.threads(matter(1))).unwrap_err();
        assert!(matches!(error, Error::Corrupt { .. }), "{name}: {error:?}");
        assert_eq!(error.code(), ErrorCode::StoreCorrupt, "{name}");
        block_on(store.close()).unwrap();
    }
}

/// A schema 7 file opens through both migrations: 0008 (`matters.addon_switches`) and this
/// one, 0009.
#[test]
fn a_schema_7_database_opens_at_9_with_no_own_picks() {
    let dir = TestDir::new("schema_7_own_picks");
    // A file as a schema 7 build left it: the first seven migrations, a matter and a thread.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute_batch(&format!(
        "BEGIN;\n{}\n{}\n{}\n{}\n{}\n{}\n{}\nPRAGMA user_version = 7;\nCOMMIT;",
        include_str!("../../src/migrations/0001_init.sql"),
        include_str!("../../src/migrations/0002_matter_details.sql"),
        include_str!("../../src/migrations/0003_thread_seen.sql"),
        include_str!("../../src/migrations/0004_thread_title_source.sql"),
        include_str!("../../src/migrations/0005_matter_skill_packs.sql"),
        include_str!("../../src/migrations/0006_matter_agent_prefs.sql"),
        include_str!("../../src/migrations/0007_pending_actions.sql"),
    ))
    .unwrap();
    let old = thread_row(1, matter(1), None);
    conn.execute_batch(&format!(
        "INSERT INTO matters (id, name, kind, folder_path, shadow_repo_path, created_ms) \
         VALUES ('{m}', 'Matter 1', 'folder', '/matters/1', '/shadow/1', 0);
         INSERT INTO threads (id, matter_id, agent_id, created_ms, updated_ms) \
         VALUES ('{t}', '{m}', 'codex-acp', {at}, {at});",
        m = matter(1),
        t = old.id,
        at = old.created.as_millis(),
    ))
    .unwrap();
    drop(conn);

    let store = dir.open();
    assert_eq!(
        block_on(store.threads(matter(1))).unwrap(),
        std::slice::from_ref(&old)
    );
    // The migrated row takes own picks like a new one.
    let kept = ThreadRow {
        own_picks: Some(picks(&[("model", "gpt-6-luna")])),
        ..old
    };
    block_on(store.commit(Commit {
        thread: Some(kept.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, i64::from(SCHEMA));
    assert_eq!(SCHEMA, 9);
    drop(conn);
    let store = dir.open();
    assert_eq!(block_on(store.threads(matter(1))).unwrap(), [kept]);
    block_on(store.close()).unwrap();
}

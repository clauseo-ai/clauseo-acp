//! A matter's model and effort picks (schema 6, `UpdateMatter::agent_prefs`): a database from
//! schema 5 opens with no prefs on its matters, prefs round-trip through a reopen, and stored
//! prefs that do not parse are corrupt.

use std::collections::BTreeMap;

use clauseo_protocol::{AgentChoiceId, AgentKind, AgentPrefs, ConfigPicks, ErrorCode, MatterKind};
use clauseo_store::{Commit, DB_FILE, Error, MatterRow, SCHEMA};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, matter, ts};

fn matter_row(n: u64, agent_prefs: Option<AgentPrefs>) -> MatterRow {
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
        skill_packs: None,
        agent_prefs,
    }
}

fn prefs(kind: AgentKind, pairs: &[(&str, &str)]) -> AgentPrefs {
    let id = |text: &str| AgentChoiceId::new(text).unwrap();
    let picks = ConfigPicks::new(pairs.iter().map(|(o, v)| (id(o), id(v))).collect()).unwrap();
    AgentPrefs(BTreeMap::from([(kind, picks)]))
}

#[test]
fn agent_prefs_round_trip() {
    let dir = TestDir::new("agent_prefs_round_trip");
    let store = dir.open();
    let rows = [
        matter_row(1, None),
        matter_row(
            2,
            Some(prefs(
                AgentKind::Claude,
                &[("model", "opus"), ("effort", "high")],
            )),
        ),
        matter_row(3, Some(AgentPrefs::default())),
    ];
    for row in &rows {
        block_on(store.commit(Commit {
            matter: Some(row.clone()),
            ..Commit::default()
        }))
        .unwrap();
    }
    block_on(store.close()).unwrap();
    let store = dir.open();
    assert_eq!(block_on(store.matters()).unwrap(), rows);
    block_on(store.close()).unwrap();
}

#[test]
fn stored_agent_prefs_that_do_not_parse_are_corrupt() {
    for (name, value) in [
        ("not_json", "{"),
        ("unknown_kind", r#"{"gemini":{"model":"x"}}"#),
        ("not_a_value", r#"{"claude":{"model":1}}"#),
    ] {
        let dir = TestDir::new(&format!("agent_prefs_corrupt_{name}"));
        let store = dir.open();
        block_on(store.commit(Commit {
            matter: Some(matter_row(1, None)),
            ..Commit::default()
        }))
        .unwrap();
        block_on(store.close()).unwrap();
        let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
        conn.execute("UPDATE matters SET agent_prefs = ?1", [value])
            .unwrap();
        drop(conn);
        let store = dir.open();
        let error = block_on(store.matters()).unwrap_err();
        assert!(matches!(error, Error::Corrupt { .. }), "{name}: {error:?}");
        assert_eq!(error.code(), ErrorCode::StoreCorrupt, "{name}");
        block_on(store.close()).unwrap();
    }
}

#[test]
fn schema_5_database_migrates_and_reads_agent_prefs_as_null() {
    let dir = TestDir::new("schema_5_agent_prefs");
    // A file as a schema 5 build left it: the first five migrations and one matter.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute_batch(&format!(
        "BEGIN;\n{}\n{}\n{}\n{}\n{}\nPRAGMA user_version = 5;\nCOMMIT;",
        include_str!("../../src/migrations/0001_init.sql"),
        include_str!("../../src/migrations/0002_matter_details.sql"),
        include_str!("../../src/migrations/0003_thread_seen.sql"),
        include_str!("../../src/migrations/0004_thread_title_source.sql"),
        include_str!("../../src/migrations/0005_matter_skill_packs.sql"),
    ))
    .unwrap();
    let old = matter_row(1, None);
    conn.execute(
        "INSERT INTO matters (id, name, kind, folder_path, shadow_repo_path, created_ms) \
         VALUES (?1, ?2, 'folder', ?3, ?4, ?5)",
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
    let rows = block_on(store.matters()).unwrap();
    assert_eq!(rows, std::slice::from_ref(&old));
    assert_eq!(rows[0].agent_prefs, None);
    // The migrated row takes prefs like a new one.
    let picked = MatterRow {
        agent_prefs: Some(prefs(AgentKind::Codex, &[("reasoning_effort", "high")])),
        ..old
    };
    block_on(store.commit(Commit {
        matter: Some(picked.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, i64::from(SCHEMA));
    assert_eq!(SCHEMA, 6);
    drop(conn);
    let store = dir.open();
    assert_eq!(block_on(store.matters()).unwrap(), [picked]);
    block_on(store.close()).unwrap();
}

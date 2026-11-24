//! A matter's own switches for skills and tools (schema 8, decision record 0040): a database from
//! schema 7 opens with none on its matters, switches round-trip through a reopen (none left is
//! stored as NULL), several matters' rows go in one commit, and a stored value that does not
//! parse reads as no switch.

use std::collections::BTreeMap;

use clauseo_protocol::{AddonKind, AddonSwitches, MatterKind};
use clauseo_store::{Commit, DB_FILE, MatterRow, SCHEMA};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, matter, ts};

fn matter_row(n: u64, addon_switches: Option<AddonSwitches>) -> MatterRow {
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
        agent_prefs: None,
        addon_switches,
    }
}

fn switches(skills: &[(&str, bool)], tools: &[(&str, bool)]) -> AddonSwitches {
    let map = |pairs: &[(&str, bool)]| -> BTreeMap<String, bool> {
        pairs
            .iter()
            .map(|(id, on)| ((*id).to_owned(), *on))
            .collect()
    };
    AddonSwitches {
        skill: map(skills),
        tool: map(tools),
    }
}

#[test]
fn switches_round_trip_and_none_left_is_null() {
    let dir = TestDir::new("addon_switches_round_trip");
    let store = dir.open();
    let on = switches(&[("pdf", true)], &[("linear", false), ("case-law", false)]);
    let rows = [matter_row(1, Some(on.clone())), matter_row(2, None)];
    // Two matters' rows in one commit: `matter` and `more_matters`.
    block_on(store.commit(Commit {
        matter: Some(rows[0].clone()),
        more_matters: vec![rows[1].clone()],
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let store = dir.open();
    assert_eq!(block_on(store.matters()).unwrap(), rows);
    // Clearing the last switch stores NULL, which reads back as none.
    let mut cleared = on;
    cleared.set(AddonKind::Skill, "pdf", None);
    cleared.set(AddonKind::Tool, "linear", None);
    cleared.set(AddonKind::Tool, "case-law", None);
    assert!(cleared.is_empty());
    block_on(store.commit(Commit {
        more_matters: vec![matter_row(1, Some(cleared))],
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    let stored: Option<String> = conn
        .query_row(
            "SELECT addon_switches FROM matters WHERE id = ?1",
            [matter(1).to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, None);
    drop(conn);
    let store = dir.open();
    assert_eq!(
        block_on(store.matters()).unwrap(),
        [matter_row(1, None), matter_row(2, None)]
    );
    block_on(store.close()).unwrap();
}

#[test]
fn a_schema_7_database_migrates_with_no_switches() {
    let dir = TestDir::new("addon_switches_schema_7");
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
    assert_eq!(
        block_on(store.matters()).unwrap(),
        std::slice::from_ref(&old)
    );
    let switched = matter_row(1, Some(switches(&[("pdf", true)], &[])));
    block_on(store.commit(Commit {
        matter: Some(switched.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, i64::from(SCHEMA));
    assert_eq!(SCHEMA, 8);
    drop(conn);
    let store = dir.open();
    assert_eq!(block_on(store.matters()).unwrap(), [switched]);
    block_on(store.close()).unwrap();
}

#[test]
fn stored_switches_that_do_not_parse_read_as_none() {
    let cases = [
        ("not_json", "not json"),
        ("not_a_bool", r#"{"skill":{"pdf":"yes"}}"#),
        ("unknown_kind", r#"{"plugin":{"pdf":true}}"#),
    ];
    for (name, value) in cases {
        let dir = TestDir::new(&format!("addon_switches_corrupt_{name}"));
        let store = dir.open();
        block_on(store.commit(Commit {
            matter: Some(matter_row(1, None)),
            ..Commit::default()
        }))
        .unwrap();
        block_on(store.close()).unwrap();
        let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
        conn.execute("UPDATE matters SET addon_switches = ?1", [value])
            .unwrap();
        drop(conn);
        let store = dir.open();
        assert_eq!(
            block_on(store.matters()).unwrap(),
            [matter_row(1, None)],
            "{name}"
        );
        block_on(store.close()).unwrap();
    }
}

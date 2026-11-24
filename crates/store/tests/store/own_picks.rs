//! A thread's own model and effort (schema 9, `ThreadRow::own_picks`): a database from schema 7
//! opens with no own picks on its threads and takes them after, own picks (per agent)
//! round-trip through a reopen, and stored own picks that do not decode read as none, or lose
//! the entries that do not.

use clauseo_protocol::{AgentChoiceId, AgentId, ConfigPicks, MatterId, MatterKind, ThreadId};
use clauseo_store::{Commit, DB_FILE, MatterRow, OwnPicks, SCHEMA, ThreadRow};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, id, matter, ts};

fn thread_row(n: u64, matter: MatterId, own_picks: Option<OwnPicks>) -> ThreadRow {
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

/// Own picks of `codex-acp` alone.
fn codex(pairs: &[(&str, &str)]) -> OwnPicks {
    OwnPicks([(AgentId::new("codex-acp").unwrap(), picks(pairs))].into())
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
            Some(codex(&[("model", "gpt-6-luna")])),
        )),
        ..matter_commit()
    }
}

#[test]
fn own_picks_round_trip_through_a_reopen() {
    let dir = TestDir::new("own_picks_round_trip");
    let store = dir.open();
    block_on(store.commit(matter_commit())).unwrap();
    let mut both = codex(&[("model", "gpt-6-luna"), ("reasoning_effort", "low")]);
    both.0.insert(
        AgentId::new("claude-acp").unwrap(),
        picks(&[("model", "haiku"), ("effort", "low")]),
    );
    let rows = [
        thread_row(3, matter(1), Some(both)),
        thread_row(2, matter(1), Some(OwnPicks::default())),
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

/// A store holding thread 1 with `value` as its stored own picks, reopened: its rows.
fn stored(name: &str, value: &str) -> Vec<ThreadRow> {
    let dir = TestDir::new(&format!("own_picks_stored_{name}"));
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
    let rows = block_on(store.threads(matter(1))).unwrap();
    block_on(store.close()).unwrap();
    rows
}

#[test]
fn stored_own_picks_that_do_not_decode_read_as_none() {
    for (name, value) in [
        ("not_json", "{"),
        ("not_an_object", "[1]"),
        ("a_string", r#""x""#),
    ] {
        let rows = stored(name, value);
        assert_eq!(rows.len(), 1, "{name}");
        assert_eq!(rows[0].own_picks, None, "{name}");
    }
}

#[test]
fn entries_of_own_picks_that_do_not_decode_are_left_out() {
    let long = "m".repeat(300);
    let nine: Vec<String> = (0..9).map(|n| format!(r#""o{n}":"v""#)).collect();
    for (name, bad) in [
        (
            "an_agent_key_that_is_no_id",
            r#""Not An Agent!":{"model":"x"}"#.to_owned(),
        ),
        (
            "a_value_that_is_no_id",
            r#""claude-acp":{"model":1}"#.to_owned(),
        ),
        (
            "more_than_eight",
            format!(r#""claude-acp":{{{}}}"#, nine.join(",")),
        ),
        (
            "an_id_too_long",
            format!(r#""claude-acp":{{"model":"{long}"}}"#),
        ),
    ] {
        let value = format!(r#"{{{bad},"codex-acp":{{"model":"gpt-6-luna"}}}}"#);
        let rows = stored(name, &value);
        assert_eq!(
            rows[0].own_picks,
            Some(codex(&[("model", "gpt-6-luna")])),
            "{name}"
        );
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
        own_picks: Some(codex(&[("model", "gpt-6-luna")])),
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

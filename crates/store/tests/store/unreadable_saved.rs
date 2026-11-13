//! A matter's saved agent picks or skill packs that do not decode (a value a newer build wrote,
//! or garbage): `matters()` still reads every row, the matter reads with no picks or no packs,
//! its row is left as it is, the other columns and matters read as before, and one WARN names
//! the matter and the column without the stored text.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentChoiceId, AgentKind, AgentPrefs, ConfigPicks, MatterKind, PackId, PackList,
};
use clauseo_store::{Commit, DB_FILE, MatterRow};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, captured_warnings, matter, ts};

/// The matter row of `n`, with the given picks and packs.
fn row(n: u64, agent_prefs: Option<AgentPrefs>, skill_packs: Option<PackList>) -> MatterRow {
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
        skill_packs,
        agent_prefs,
    }
}

fn prefs() -> AgentPrefs {
    let id = |text: &str| AgentChoiceId::new(text).unwrap();
    let picks = ConfigPicks::new(BTreeMap::from([(id("model"), id("opus"))])).unwrap();
    AgentPrefs(BTreeMap::from([(AgentKind::Claude, picks)]))
}

fn packs(ids: &[&str]) -> PackList {
    PackList::new(ids.iter().map(|id| PackId::new(id).unwrap()).collect()).unwrap()
}

#[test]
fn an_unreadable_saved_value_reads_as_the_default_and_the_rest_stays() {
    // Matter numbers no other test uses, so the WARN lines of this test are its own.
    let (good, bad_prefs, bad_packs) = (41, 42, 43);
    let bad_prefs_text = r#"{"gemini":{"model":"pick-7731"}}"#;
    let bad_packs_text = "not json";
    let dir = TestDir::new("unreadable_saved");
    let warnings = captured_warnings();
    let store = dir.open();
    let written = [
        row(good, Some(prefs()), Some(packs(&["contracts", "tax-2026"]))),
        row(bad_prefs, Some(prefs()), Some(packs(&["contracts"]))),
        row(bad_packs, Some(prefs()), Some(packs(&["tax-2026"]))),
    ];
    for matter_row in &written {
        block_on(store.commit(Commit {
            matter: Some(matter_row.clone()),
            ..Commit::default()
        }))
        .unwrap();
    }
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute(
        "UPDATE matters SET agent_prefs = ?1 WHERE id = ?2",
        (bad_prefs_text, matter(bad_prefs).to_string()),
    )
    .unwrap();
    conn.execute(
        "UPDATE matters SET skill_packs = ?1 WHERE id = ?2",
        (bad_packs_text, matter(bad_packs).to_string()),
    )
    .unwrap();
    drop(conn);

    let store = dir.open();
    let rows = block_on(store.matters()).unwrap();
    assert_eq!(
        rows,
        [
            written[0].clone(),
            MatterRow {
                agent_prefs: None,
                ..written[1].clone()
            },
            MatterRow {
                skill_packs: None,
                ..written[2].clone()
            },
        ]
    );
    block_on(store.close()).unwrap();

    // The stored values are left as they were: a read repairs nothing.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    let stored_prefs: String = conn
        .query_row(
            "SELECT agent_prefs FROM matters WHERE id = ?1",
            [matter(bad_prefs).to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let stored_packs: String = conn
        .query_row(
            "SELECT skill_packs FROM matters WHERE id = ?1",
            [matter(bad_packs).to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_prefs, bad_prefs_text);
    assert_eq!(stored_packs, bad_packs_text);
    drop(conn);

    // One WARN per bad value: it names the matter and the column, says the default is used,
    // and holds neither the stored text nor the decoder's message.
    let lines = warnings.lock().unwrap().clone();
    let about = |n: u64, column: &str| {
        lines
            .iter()
            .filter(|line| line.contains(&matter(n).to_string()) && line.contains(column))
            .count()
    };
    assert_eq!(about(bad_prefs, "agent_prefs"), 1, "{lines:?}");
    assert_eq!(about(bad_packs, "skill_packs"), 1, "{lines:?}");
    assert_eq!(about(good, "agent_prefs"), 0, "{lines:?}");
    assert_eq!(about(good, "skill_packs"), 0, "{lines:?}");
    let ours: Vec<&String> = lines
        .iter()
        .filter(|line| {
            line.contains(&matter(bad_prefs).to_string())
                || line.contains(&matter(bad_packs).to_string())
        })
        .collect();
    assert_eq!(ours.len(), 2, "{lines:?}");
    for line in &ours {
        assert!(line.contains("default"), "{line}");
        assert!(
            !line.contains("pick-7731"),
            "the stored text leaked: {line}"
        );
        assert!(!line.contains("not json"), "the stored text leaked: {line}");
        assert!(
            !line.contains("unknown variant"),
            "the decoder's message leaked: {line}"
        );
    }
}

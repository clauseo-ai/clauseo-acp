//! A matter's skill packs (schema 5, decision record 0040): a database from schema 4 opens with
//! no list on its matters, a list (empty included) round-trips through a reopen, and a stored
//! list that does not parse, or breaks the list's rules, is corrupt.

use clauseo_protocol::{ErrorCode, MatterKind, PackId, PackList};
use clauseo_store::{Commit, DB_FILE, Error, MatterRow, SCHEMA};
use futures::executor::block_on;
use rusqlite::Connection;

use crate::common::{TestDir, matter, ts};

fn matter_row(n: u64, skill_packs: Option<PackList>) -> MatterRow {
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
    }
}

fn packs(ids: &[&str]) -> PackList {
    PackList::new(ids.iter().map(|id| PackId::new(id).unwrap()).collect()).unwrap()
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn skill_packs_round_trip_including_empty() {
    let dir = TestDir::new("skill_packs_round_trip");
    let store = dir.open();
    let rows = [
        matter_row(1, Some(packs(&["india-litigation", "tax-2026"]))),
        matter_row(2, Some(PackList::default())),
        matter_row(3, None),
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
    // A second put replaces the list, and clearing it stores NULL again.
    let changed = [
        MatterRow {
            skill_packs: None,
            ..rows[0].clone()
        },
        MatterRow {
            skill_packs: Some(packs(&["contracts"])),
            ..rows[1].clone()
        },
    ];
    for row in &changed {
        block_on(store.commit(Commit {
            matter: Some(row.clone()),
            ..Commit::default()
        }))
        .unwrap();
    }
    block_on(store.close()).unwrap();
    let store = dir.open();
    assert_eq!(
        block_on(store.matters()).unwrap(),
        [changed[0].clone(), changed[1].clone(), rows[2].clone()]
    );
    block_on(store.close()).unwrap();
}

#[test]
fn a_stored_skill_packs_that_does_not_parse_is_corrupt() {
    let too_many = format!(
        "[{}]",
        (0..33)
            .map(|n| format!("\"pack-{n}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    let cases = [
        ("not_json", "not json".to_owned()),
        ("not_an_array", "{\"packs\":[]}".to_owned()),
        ("bad_id", "[\"Not An Id\"]".to_owned()),
        ("reserved_id", "[\"clauseo\"]".to_owned()),
        ("thirty_three", too_many),
        ("duplicate", "[\"contracts\",\"contracts\"]".to_owned()),
    ];
    for (name, value) in cases {
        let dir = TestDir::new(&format!("skill_packs_corrupt_{name}"));
        let store = dir.open();
        block_on(store.commit(Commit {
            matter: Some(matter_row(1, None)),
            ..Commit::default()
        }))
        .unwrap();
        block_on(store.close()).unwrap();
        let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
        conn.execute("UPDATE matters SET skill_packs = ?1", [&value])
            .unwrap();
        drop(conn);
        let store = dir.open();
        let error = block_on(store.matters()).unwrap_err();
        assert!(
            matches!(&error, Error::Corrupt { detail } if detail.contains("skill packs")),
            "{name}: {error:?}"
        );
        assert_eq!(error.code(), ErrorCode::StoreCorrupt);
        block_on(store.close()).unwrap();
    }
}

#[test]
fn schema_4_database_migrates_and_reads_skill_packs_as_null() {
    let dir = TestDir::new("schema_4_skill_packs");
    // A file as a schema 4 build left it: the first four migrations and one matter.
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    conn.execute_batch(&format!(
        "BEGIN;\n{}\n{}\n{}\n{}\nPRAGMA user_version = 4;\nCOMMIT;",
        include_str!("../../src/migrations/0001_init.sql"),
        include_str!("../../src/migrations/0002_matter_details.sql"),
        include_str!("../../src/migrations/0003_thread_seen.sql"),
        include_str!("../../src/migrations/0004_thread_title_source.sql"),
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
    assert_eq!(rows[0].skill_packs, None);
    // The migrated row takes a list like a new one.
    let narrowed = MatterRow {
        skill_packs: Some(packs(&["contracts"])),
        ..old
    };
    block_on(store.commit(Commit {
        matter: Some(narrowed.clone()),
        ..Commit::default()
    }))
    .unwrap();
    block_on(store.close()).unwrap();
    let conn = Connection::open(dir.path().join(DB_FILE)).unwrap();
    assert_eq!(user_version(&conn), i64::from(SCHEMA));
    assert_eq!(SCHEMA, 5);
    drop(conn);
    let store = dir.open();
    assert_eq!(block_on(store.matters()).unwrap(), [narrowed]);
    block_on(store.close()).unwrap();
}

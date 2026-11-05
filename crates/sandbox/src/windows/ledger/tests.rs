use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use super::write::replace_with;
use super::*;
use crate::test_support::TempDir;
use crate::windows::test_world::{MATTER_SID, USER_SID, matter_one, matter_two};

fn grant(path: &str, sid: &str, rights: Rights) -> Grant {
    Grant {
        path: PathBuf::from(path),
        sid: Sid::new(sid).unwrap(),
        rights,
        inherit: Inherit::Tree,
        kind: PathKind::Matter,
    }
}

/// A pair on one folder, and a deny.
fn grants(folder: &str) -> Vec<Grant> {
    vec![
        grant(folder, USER_SID, Rights::Modify),
        grant(folder, MATTER_SID, Rights::Modify),
        grant(&format!("{folder}/.git"), MATTER_SID, Rights::DenyWrite),
    ]
}

fn states(ledger: &Ledger) -> Vec<(u64, EntryState)> {
    ledger.entries().map(|e| (e.id.0, e.state)).collect()
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn the_ledger_lives_in_the_data_folders_sandbox_folder() {
    let path = ledger_path(Path::new("/data"));
    assert_eq!(
        path,
        Path::new("/data").join("sandbox").join("acl-ledger.json")
    );
}

#[test]
fn a_missing_ledger_is_empty_and_stays_unwritten() {
    let temp = TempDir::new("ledger-missing");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.entries().count(), 0);
    assert_eq!(
        ledger.reconcile(|_| Ok(true)).unwrap(),
        Reconciled::default()
    );
    assert_eq!(
        ledger.forget_matter(matter_one(), |_| Ok(())).unwrap(),
        Forgotten::default()
    );
    ledger.mark_applied(&[EntryId(1)]).unwrap();
    assert!(!path.exists());
    assert!(!temp.path().join("sandbox").exists());
}

#[test]
fn entries_are_pending_then_applied() {
    let temp = TempDir::new("ledger-pending-applied");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    let ids = ledger.record_pending(matter_one(), &grants("/m1")).unwrap();
    assert_eq!(ids, [EntryId(1), EntryId(2), EntryId(3)]);
    // Written before any permission entry would be: the file already lists them, pending.
    let reopened = Ledger::open(&path).unwrap();
    assert_eq!(
        states(&reopened),
        [
            (1, EntryState::Pending),
            (2, EntryState::Pending),
            (3, EntryState::Pending)
        ]
    );
    let text = String::from_utf8(read(&path)).unwrap();
    assert!(text.contains("\"pending\""), "{text}");
    ledger.mark_applied(&ids[..2]).unwrap();
    let reopened = Ledger::open(&path).unwrap();
    assert_eq!(
        states(&reopened),
        [
            (1, EntryState::Applied),
            (2, EntryState::Applied),
            (3, EntryState::Pending)
        ]
    );
    let entry = reopened.entries().next().unwrap();
    assert_eq!(entry.matter, matter_one());
    assert_eq!(entry.grant(), grants("/m1")[0]);
}

#[test]
fn a_grant_listed_already_keeps_its_entry_and_the_file() {
    let temp = TempDir::new("ledger-relisted");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    let first = ledger.record_pending(matter_one(), &grants("/m1")).unwrap();
    ledger.mark_applied(&first).unwrap();
    let before = read(&path);
    let again = ledger.record_pending(matter_one(), &grants("/m1")).unwrap();
    assert_eq!(again, first);
    assert_eq!(read(&path), before);
    // The same grant for another matter is that matter's own entry.
    let other = ledger
        .record_pending(matter_two(), &grants("/m1")[..1])
        .unwrap();
    assert_eq!(other, [EntryId(4)]);
}

#[test]
fn reconcile_keeps_a_pending_entry_whose_ace_is_there_and_drops_one_whose_isnt() {
    let temp = TempDir::new("ledger-reconcile");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    let applied = ledger
        .record_pending(matter_one(), &grants("/m0")[..1])
        .unwrap();
    ledger.mark_applied(&applied).unwrap();
    // A crash after the pending entries: 2 has its ACE, 3 doesn't, 4 can't be checked.
    let mut pending = grants("/m1");
    pending.truncate(2);
    pending.push(grant("/m1/x", MATTER_SID, Rights::DenyWrite));
    ledger.record_pending(matter_one(), &pending).unwrap();
    let mut asked = Vec::new();
    let done = ledger
        .reconcile(|entry| {
            asked.push(entry.id.0);
            match entry.id.0 {
                2 => Ok(true),
                3 => Ok(false),
                _ => Err(io::Error::other("the folder is held")),
            }
        })
        .unwrap();
    assert_eq!(asked, [2, 3, 4], "an applied entry isn't checked");
    assert_eq!(
        done,
        Reconciled {
            applied: 1,
            dropped: 1,
            unchecked: 1
        }
    );
    let reopened = Ledger::open(&path).unwrap();
    assert_eq!(
        states(&reopened),
        [
            (1, EntryState::Applied),
            (2, EntryState::Applied),
            (4, EntryState::Pending)
        ]
    );
    // Numbers are never reused, so a dropped entry's number can't name a new one.
    let next = ledger
        .record_pending(matter_one(), &grants("/m9")[..1])
        .unwrap();
    assert_eq!(next, [EntryId(5)]);
}

#[test]
fn reconcile_with_nothing_to_change_leaves_the_file() {
    let temp = TempDir::new("ledger-reconcile-nothing");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    let ids = ledger.record_pending(matter_one(), &grants("/m1")).unwrap();
    ledger.mark_applied(&ids).unwrap();
    let before = read(&path);
    let done = ledger
        .reconcile(|_| panic!("no pending entry to check"))
        .unwrap();
    assert_eq!(done, Reconciled::default());
    assert_eq!(read(&path), before);
}

#[test]
fn forget_matter_drops_exactly_that_matters_entries() {
    let temp = TempDir::new("ledger-forget");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    let one = ledger.record_pending(matter_one(), &grants("/m1")).unwrap();
    let two = ledger.record_pending(matter_two(), &grants("/m2")).unwrap();
    ledger.mark_applied(&one).unwrap();
    let listed: Vec<EntryId> = ledger
        .matter_entries(matter_one())
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(listed, one);
    let mut removed = Vec::new();
    let done = ledger
        .forget_matter(matter_one(), |entry| {
            removed.push(entry.id);
            Ok(())
        })
        .unwrap();
    assert_eq!(removed, one);
    assert_eq!(
        done,
        Forgotten {
            removed: 3,
            kept: 0
        }
    );
    let reopened = Ledger::open(&path).unwrap();
    let left: Vec<EntryId> = reopened.entries().map(|e| e.id).collect();
    assert_eq!(left, two);
    assert!(reopened.entries().all(|e| e.matter == matter_two()));
}

#[test]
fn forget_matter_keeps_an_entry_whose_removal_failed() {
    let temp = TempDir::new("ledger-forget-retry");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    let ids = ledger.record_pending(matter_one(), &grants("/m1")).unwrap();
    let done = ledger
        .forget_matter(matter_one(), |entry| {
            if entry.id == ids[1] {
                Err(io::Error::other("WRITE_DAC was taken away"))
            } else {
                Ok(())
            }
        })
        .unwrap();
    assert_eq!(
        done,
        Forgotten {
            removed: 2,
            kept: 1
        }
    );
    let left: Vec<EntryId> = Ledger::open(&path)
        .unwrap()
        .entries()
        .map(|e| e.id)
        .collect();
    assert_eq!(left, [ids[1]]);
}

#[test]
fn drop_entries_drops_those_numbers_alone() {
    let temp = TempDir::new("ledger-drop");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    let ids = ledger.record_pending(matter_one(), &grants("/m1")).unwrap();
    ledger.drop_entries(&[ids[0], ids[2], EntryId(99)]).unwrap();
    let left: Vec<EntryId> = Ledger::open(&path)
        .unwrap()
        .entries()
        .map(|e| e.id)
        .collect();
    assert_eq!(left, [ids[1]]);
}

#[test]
fn a_failed_replace_leaves_the_old_file_whole_and_no_temporary_file() {
    let temp = TempDir::new("ledger-atomic");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    ledger.record_pending(matter_one(), &grants("/m1")).unwrap();
    let before = read(&path);
    let error = replace_with(&path, b"{\"version\": 1, \"entries\": [", |file, bytes| {
        file.write_all(&bytes[..10])?;
        Err(io::Error::other("the disk is full"))
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "the disk is full");
    assert_eq!(read(&path), before);
    let names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["acl-ledger.json"]);
    // The ledger in memory still matches the file, and writes again.
    let reopened = Ledger::open(&path).unwrap();
    assert_eq!(reopened.entries().count(), 3);
    ledger.mark_applied(&[EntryId(1)]).unwrap();
}

#[test]
fn a_temporary_file_left_by_a_crash_is_replaced() {
    let temp = TempDir::new("ledger-stale-temp");
    temp.file("sandbox/.acl-ledger.json.tmp", "{\"half");
    let path = ledger_path(temp.path());
    let mut ledger = Ledger::open(&path).unwrap();
    ledger.record_pending(matter_one(), &grants("/m1")).unwrap();
    assert_eq!(Ledger::open(&path).unwrap().entries().count(), 3);
    assert!(!temp.path().join("sandbox/.acl-ledger.json.tmp").exists());
}

#[test]
fn a_file_where_the_folder_goes_is_refused() {
    let temp = TempDir::new("ledger-folder-is-file");
    let mut ledger = Ledger::open(ledger_path(temp.path())).unwrap();
    temp.file("sandbox", "not a folder");
    let error = ledger
        .record_pending(matter_one(), &grants("/m1"))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        ledger.entries().count(),
        0,
        "memory matches the file after a failed write"
    );
}

#[test]
fn entries_this_version_cant_read_are_kept_and_left_alone() {
    let temp = TempDir::new("ledger-unknown");
    let one = matter_one();
    let text = format!(
        r#"{{
  "version": 1,
  "next_id": 3,
  "written_by": "a later Clauseo",
  "entries": [
    {{"id": 1, "matter": "{one}", "state": "applied", "path": "/m1", "sid": "{MATTER_SID}",
      "rights": "modify", "inherit": "tree", "kind": "matter", "note": "kept"}},
    {{"id": 2, "matter": "{one}", "path": "/m1/old", "sid": "{MATTER_SID}"}},
    {{"id": 7, "matter": "{one}", "state": "applied", "path": "/m1", "sid": "{MATTER_SID}",
      "rights": "full_control", "inherit": "tree", "kind": "matter"}},
    {{"id": 8, "matter": "not a matter id", "state": "pending", "path": "/m1",
      "sid": "{MATTER_SID}", "rights": "modify", "inherit": "tree", "kind": "matter"}},
    {{"id": 9, "matter": "{one}", "state": "pending", "path": "/m1", "sid": "Users",
      "rights": "modify", "inherit": "tree", "kind": "a_newer_kind"}},
    "not an object",
    42
  ]
}}"#
    );
    let path = temp.file("sandbox/acl-ledger.json", &text);
    let mut ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.entries().count(), 1);
    assert_eq!(ledger.unknown_entries(), 6);
    // Nothing acts on an unread entry: reconcile and forget pass over them.
    assert_eq!(
        ledger
            .reconcile(|_| panic!("unread entries aren't checked"))
            .unwrap(),
        Reconciled::default()
    );
    let done = ledger.forget_matter(one, |entry| {
        assert_eq!(entry.id, EntryId(1));
        Err(io::Error::other("keep it"))
    });
    assert_eq!(
        done.unwrap(),
        Forgotten {
            removed: 0,
            kept: 1
        }
    );
    // A new entry takes a number past every one in the file, and the rewrite keeps the rest.
    let ids = ledger.record_pending(one, &grants("/m5")[..1]).unwrap();
    assert_eq!(ids, [EntryId(10)]);
    let written: serde_json::Value = serde_json::from_slice(&read(&path)).unwrap();
    let before: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = written["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 8);
    assert_eq!(entries[..7], before["entries"].as_array().unwrap()[..]);
    assert_eq!(written["written_by"], "a later Clauseo");
    assert_eq!(entries[0]["note"], "kept");
}

#[test]
fn a_ledger_from_a_newer_version_or_not_json_is_refused_and_left_alone() {
    let temp = TempDir::new("ledger-refused");
    for text in [
        r#"{"version": 2, "entries": []}"#,
        r#"{"version": "one", "entries": []}"#,
        r#"{"version": 1, "entries": {}}"#,
        "[]",
        "{\"version\": 1, \"entries\": [",
        "",
    ] {
        let path = temp.file("sandbox/acl-ledger.json", text);
        let error = Ledger::open(&path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{text}");
        assert_eq!(read(&path), text.as_bytes());
    }
}

#[test]
fn a_ledger_without_a_version_or_entries_reads_as_version_one() {
    let temp = TempDir::new("ledger-bare");
    let path = temp.file("sandbox/acl-ledger.json", "{}");
    let mut ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.entries().count(), 0);
    assert_eq!(
        ledger.record_pending(matter_one(), &grants("/m1")).unwrap()[0],
        EntryId(1)
    );
    let written: serde_json::Value = serde_json::from_slice(&read(&path)).unwrap();
    assert_eq!(written["version"], LEDGER_VERSION);
}

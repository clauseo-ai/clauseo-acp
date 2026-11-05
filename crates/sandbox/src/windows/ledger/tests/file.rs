//! The ledger as read back: entries set aside, whom an entry may name, and files refused.

use std::io;

use super::*;

#[test]
fn entries_this_version_cant_read_or_trust_are_set_aside() {
    let temp = TempDir::new("ledger-set-aside");
    let one = matter_one();
    let home = abs("home");
    let home = serde_json::to_string(&home).unwrap();
    let m1 = serde_json::to_string(&abs("m1")).unwrap();
    let text = format!(
        r#"{{
  "version": 1,
  "next_id": 3,
  "written_by": "a later Clauseo",
  "entries": [
    {{"id": 1, "matter": "{one}", "state": "applied", "path": {m1}, "sid": "{MATTER_SID}",
      "rights": "modify", "inherit": "tree", "kind": "matter", "note": "kept"}},
    {{"id": 2, "matter": "{one}", "path": {m1}, "sid": "{MATTER_SID}"}},
    {{"id": 7, "matter": "{one}", "state": "applied", "path": {m1}, "sid": "{MATTER_SID}",
      "rights": "full_control", "inherit": "tree", "kind": "matter"}},
    {{"id": 8, "matter": "not a matter id", "state": "pending", "path": {m1},
      "sid": "{MATTER_SID}", "rights": "modify", "inherit": "tree", "kind": "matter"}},
    {{"id": 9, "matter": "{one}", "state": "pending", "path": {m1}, "sid": "Users",
      "rights": "modify", "inherit": "tree", "kind": "a_newer_kind"}},
    {{"id": 11, "matter": "{one}", "state": "applied", "path": {home}, "sid": "S-1-5-32-544",
      "rights": "modify", "inherit": "tree", "kind": "matter"}},
    {{"id": 12, "matter": "{one}", "state": "applied", "path": {home}, "sid": "S-1-1-0",
      "rights": "modify", "inherit": "tree", "kind": "matter"}},
    {{"id": 13, "matter": "{one}", "state": "applied", "path": {home}, "sid": "S-1-5-18",
      "rights": "modify", "inherit": "tree", "kind": "matter"}},
    {{"id": 14, "matter": "{one}", "state": "applied", "path": {home},
      "sid": "S-1-5-21-7-8-9-1002", "rights": "modify", "inherit": "tree", "kind": "matter"}},
    {{"id": 15, "matter": "{one}", "state": "applied", "path": "relative\\m1",
      "sid": "{MATTER_SID}", "rights": "modify", "inherit": "tree", "kind": "matter"}},
    "not an object",
    42
  ]
}}"#
    );
    let path = temp.file("sandbox/acl-ledger.json", &text);
    let mut ledger = open(&path);
    assert_eq!(ids(&ledger), [EntryId(1)]);
    assert_eq!(ledger.set_aside_entries(), 11);
    // Nothing acts on a set-aside entry: reconcile and forget pass over them, so the tampered
    // entries naming Administrators, Everyone, SYSTEM or the lawyer's own domain on the home
    // folder are never handed out for removal.
    let done = ledger
        .reconcile(|_| panic!("set-aside entries aren't checked"))
        .unwrap();
    assert_eq!(done, Reconciled::default());
    let mut handed = Vec::new();
    let done = ledger.forget_matter(one, |entry| {
        handed.push(entry.id);
        Err(io::Error::other("keep it"))
    });
    assert_eq!(
        done.unwrap(),
        Forgotten {
            removed: 0,
            kept: 1,
            shared: 0
        }
    );
    assert_eq!(handed, [EntryId(1)]);
    assert_eq!(ledger.matter_entries(one).len(), 1);
    // A new entry takes a number past every one in the file, and the rewrite keeps the rest.
    let mut plan = plan_for(one, "m5");
    ledger.record_pending(&mut plan).unwrap();
    assert_eq!(plan.entries()[0], EntryId(16));
    let written: serde_json::Value = serde_json::from_slice(&read(&path)).unwrap();
    let before: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = written["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 15);
    // The entry whose removal failed was set back to pending before the removal (M4).
    assert_eq!(entries[0]["state"], "pending");
    assert_eq!(entries[1..12], before["entries"].as_array().unwrap()[1..]);
    assert_eq!(written["written_by"], "a later Clauseo");
    assert_eq!(entries[0]["note"], "kept");
}

#[test]
fn the_sandbox_user_and_matter_sids_are_trusted() {
    let temp = TempDir::new("ledger-trusted");
    let path = ledger_path(temp.path());
    let mut ledger = open(&path);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    let reopened = open(&path);
    let sids: Vec<&str> = reopened.entries().map(|e| e.sid.as_str()).collect();
    assert_eq!(sids, [USER_SID, MATTER_SID, MATTER_SID]);
    // With the matter's domain reserved (say, it is the lawyer's), its entries are set aside.
    let lawyer = Reserved::new([Sid::parse(MATTER_SID).unwrap()]);
    let strict = Ledger::open(&path, &trusted(), &lawyer).unwrap();
    assert_eq!(strict.entries().count(), 1);
    assert_eq!(strict.set_aside_entries(), 2);
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
        let error = Ledger::open(&path, &trusted(), &reserved()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{text}");
        assert_eq!(read(&path), text.as_bytes());
    }
}

#[test]
fn a_ledger_without_a_version_or_entries_reads_as_version_one() {
    let temp = TempDir::new("ledger-bare");
    let path = temp.file("sandbox/acl-ledger.json", "{}");
    let mut ledger = open(&path);
    assert_eq!(ledger.entries().count(), 0);
    let mut plan = plan_for(matter_one(), "m1");
    ledger.record_pending(&mut plan).unwrap();
    assert_eq!(plan.entries()[0], EntryId(1));
    let written: serde_json::Value = serde_json::from_slice(&read(&path)).unwrap();
    assert_eq!(written["version"], LEDGER_VERSION);
}

#[cfg(unix)]
#[test]
fn a_ledger_or_lock_file_with_a_second_link_is_refused_before_any_change() {
    // `write::check_owned_file` runs on the lock file before the lock and on the ledger under
    // it; elsewhere than Windows it refuses a link or a second hard link.
    for name in [LEDGER_FILE, LEDGER_LOCK] {
        let temp = TempDir::new("ledger-second-link");
        let path = ledger_path(temp.path());
        let mut ledger = open(&path);
        let mut plan = plan_for(matter_one(), "m1");
        ledger.record_pending(&mut plan).unwrap();
        let outside = temp.file("outside.json", "{}");
        let target = temp.path().join("sandbox").join(name);
        crate::test_support::remove(&target);
        temp.hard_link(&format!("sandbox/{name}"), &outside);
        let before = read(&outside);
        let mut next = plan_for(matter_two(), "m2");
        let error = ledger.record_pending(&mut next).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{name}");
        assert_eq!(read(&outside), before, "{name}");
        assert_eq!(next.entries(), [] as [EntryId; 0]);
    }
}

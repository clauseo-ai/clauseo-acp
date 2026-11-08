use super::*;

fn owed_of(ledger: &Ledger) -> Vec<(MatterId, PathBuf, Stuck)> {
    let owed = ledger.reclaims_owed().into_iter();
    owed.map(|o| (o.matter, o.path, o.why)).collect()
}

/// A refused launch's clean-up is written under the lock, once per item, read back by a fresh
/// open, and cleared by matter only.
#[test]
fn a_clean_up_owed_is_recorded_once_and_cleared_by_its_matter() {
    let temp = TempDir::new("ledger-reclaims");
    let path = ledger_path(temp.path());
    let (one, two) = (matter_one(), matter_two());
    let mut ledger = open(&path);
    ledger
        .owe_reclaim(one, abs("m1/doc.txt"), Stuck::PoolOwnedChild)
        .unwrap();
    ledger
        .owe_reclaim(one, abs("m1/doc.txt"), Stuck::PoolOwnedChild)
        .unwrap();
    ledger
        .owe_reclaim(two, abs("m2"), Stuck::PoolOwnedRoot)
        .unwrap();
    let want = vec![
        (one, abs("m1/doc.txt"), Stuck::PoolOwnedChild),
        (two, abs("m2"), Stuck::PoolOwnedRoot),
    ];
    assert_eq!(owed_of(&open(&path)), want);

    ledger.clear_reclaims(one).unwrap();
    assert_eq!(owed_of(&open(&path)), want[1..]);
    // Nothing listed for it: nothing read or written.
    let before = read(&path);
    ledger.clear_reclaims(one).unwrap();
    assert_eq!(read(&path), before);
    ledger.clear_reclaims(two).unwrap();
    let written: serde_json::Value = serde_json::from_slice(&read(&path)).unwrap();
    assert!(written.get("reclaims").is_none(), "{written}");
}

#[test]
fn a_relative_path_is_refused_and_nothing_written() {
    let temp = TempDir::new("ledger-reclaims-relative");
    let path = ledger_path(temp.path());
    let error = open(&path)
        .owe_reclaim(matter_one(), PathBuf::from("m1"), Stuck::ChildShut)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(!path.exists());
}

/// One this version can't read, or with a relative path, is kept as it was and never cleared.
#[test]
fn a_clean_up_this_version_cant_read_is_kept() {
    let temp = TempDir::new("ledger-reclaims-kept");
    let one = matter_one();
    let text = format!(
        r#"{{
  "version": 1,
  "next_id": 1,
  "entries": [],
  "reclaims": [
    {{"matter": "{one}", "path": "relative", "why": "child_shut"}},
    {{"matter": "{one}", "path": "{abs}", "why": "a newer kind"}}
  ]
}}"#,
        abs = abs("m1").display().to_string().replace('\\', "\\\\"),
    );
    let path = temp.file("sandbox/acl-ledger.json", &text);
    let mut ledger = open(&path);
    assert_eq!(ledger.reclaims_owed(), []);
    ledger
        .owe_reclaim(one, abs("m1/doc.txt"), Stuck::ChildShut)
        .unwrap();
    ledger.clear_reclaims(one).unwrap();
    let written: serde_json::Value = serde_json::from_slice(&read(&path)).unwrap();
    let before: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(written["reclaims"], before["reclaims"]);
}

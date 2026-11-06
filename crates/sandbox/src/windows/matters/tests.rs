//! `matters.json`: matter SIDs, the file and its lock.

use super::*;
use crate::test_support::TempDir;

mod pool;

/// A fresh matter id: a v7 UUID from random bytes, enough for distinct test matters.
fn matter() -> MatterId {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).unwrap();
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = |range: std::ops::Range<usize>| -> String {
        bytes[range]
            .iter()
            .flat_map(|byte| [byte >> 4, byte & 0xf])
            .map(|nibble| char::from_digit(u32::from(nibble), 16).unwrap())
            .collect()
    };
    let text = format!(
        "{}-{}-{}-{}-{}",
        hex(0..4),
        hex(4..6),
        hex(6..8),
        hex(8..10),
        hex(10..16)
    );
    text.parse().unwrap()
}

#[test]
fn two_matters_get_different_sids_and_keep_them_across_reloads() {
    let temp = TempDir::new("matter-sids");
    let (a, b) = (matter(), matter());
    let sid_a = matter_sid(temp.path(), &a).unwrap();
    let sid_b = matter_sid(temp.path(), &b).unwrap();
    assert_ne!(sid_a, sid_b);
    assert_eq!(matter_sid(temp.path(), &a).unwrap(), sid_a);
    let loaded = MatterSids::load(temp.path()).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded.get(&a), Some(&sid_a));
    assert_eq!(loaded.get(&b), Some(&sid_b));
}

#[test]
fn the_file_is_replaced_whole_and_reloads() {
    let temp = TempDir::new("matter-sids-file");
    let a = matter();
    let mut sids = MatterSids::load(temp.path()).unwrap();
    assert!(sids.is_empty());
    let sid = sids.sid_for(&a).unwrap();
    let folder = temp.path().join(SANDBOX_DIR);
    let text = std::fs::read_to_string(folder.join(MATTERS_FILE)).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["version"], 2);
    assert_eq!(json["matters"][a.to_string()]["sid"], sid.as_str());
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [MATTERS_FILE, MATTERS_LOCK],
        "no temporary file is left"
    );
    let b = matter();
    let sid_b = sids.sid_for(&b).unwrap();
    let again = MatterSids::load(temp.path()).unwrap();
    assert_eq!((again.get(&a), again.get(&b)), (Some(&sid), Some(&sid_b)));
}

#[test]
fn a_bad_file_is_refused_and_left_as_it_is() {
    let temp = TempDir::new("matter-sids-bad");
    let a = matter().to_string();
    let b = matter().to_string();
    let sid = "S-1-5-21-1-2-3-1000";
    for bad in [
        "not json".to_owned(),
        r#"{"version":3,"matters":{}}"#.to_owned(),
        format!(r#"{{"version":1,"matters":{{"{a}":"S-1-5-32-544"}}}}"#),
        format!(r#"{{"version":1,"matters":{{"{a}":"{sid}","{b}":"{sid}"}}}}"#),
        format!(r#"{{"version":1,"matters":{{"not-a-matter":"{sid}"}}}}"#),
        r#"{"version":1,"matters":{},"extra":1}"#.to_owned(),
    ] {
        let path = temp.file(&format!("{SANDBOX_DIR}/{MATTERS_FILE}"), &bad);
        assert!(MatterSids::load(temp.path()).is_err(), "{bad}");
        assert!(matter_sid(temp.path(), &matter()).is_err(), "{bad}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }
}

#[test]
fn a_repeated_matter_is_refused() {
    let temp = TempDir::new("matter-sids-repeat");
    let a = matter();
    let path = format!("{SANDBOX_DIR}/{MATTERS_FILE}");
    // The review's repro, without a version, and the same with one.
    let entries = format!(r#""{a}":"S-1-5-21-1-2-3-1000","{a}":"S-1-5-21-4-5-6-1000""#);
    temp.file(&path, &format!(r#"{{"matters":{{{entries}}}}}"#));
    assert!(MatterSids::load(temp.path()).is_err());
    temp.file(
        &path,
        &format!(r#"{{"version":1,"matters":{{{entries}}}}}"#),
    );
    let Err(SetupError::Failed(why)) = MatterSids::load(temp.path()) else {
        panic!("a repeated matter loaded");
    };
    assert!(why.contains("listed twice"), "{why}");
}

#[test]
fn a_sid_of_or_beside_a_machine_account_is_refused() {
    let temp = TempDir::new("matter-sids-reserved");
    let a = matter();
    let lawyer = Sid::parse("S-1-5-21-7-8-9-1001").unwrap();
    let reserved = || Reserved::new([lawyer.clone()]);
    let path = format!("{SANDBOX_DIR}/{MATTERS_FILE}");
    for sid in ["S-1-5-21-7-8-9-1001", "S-1-5-21-7-8-9-424242"] {
        temp.file(
            &path,
            &format!(r#"{{"version":1,"matters":{{"{a}":"{sid}"}}}}"#),
        );
        assert!(
            MatterSids::load_with(temp.path(), reserved()).is_err(),
            "{sid}"
        );
        assert!(
            MatterSids::load_with(temp.path(), Reserved::default()).is_ok(),
            "{sid}"
        );
    }
    temp.file(
        &path,
        &format!(r#"{{"version":1,"matters":{{"{a}":"S-1-5-21-7-8-10-1001"}}}}"#),
    );
    assert!(MatterSids::load_with(temp.path(), reserved()).is_ok());
}

#[test]
fn new_sids_are_admitted_by_the_reservations() {
    let temp = TempDir::new("matter-sids-new-reserved");
    let lawyer = Sid::parse("S-1-5-21-7-8-9-1001").unwrap();
    let reserved = Reserved::new([lawyer]);
    let mut sids = MatterSids::load_with(temp.path(), reserved.clone()).unwrap();
    for _ in 0..50 {
        let sid = sids.sid_for(&matter()).unwrap();
        assert!(reserved.admits(&sid), "{sid}");
    }
}

#[test]
fn two_instances_adding_matters_keep_both() {
    let temp = TempDir::new("matter-sids-two");
    let mut first = MatterSids::load(temp.path()).unwrap();
    let mut second = MatterSids::load(temp.path()).unwrap();
    let (a, b) = (matter(), matter());
    let sid_a = first.sid_for(&a).unwrap();
    let sid_b = second.sid_for(&b).unwrap();
    // The second re-read the file under the lock, so it saw A before adding B.
    assert_eq!(second.get(&a), Some(&sid_a));
    let again = MatterSids::load(temp.path()).unwrap();
    assert_eq!((again.get(&a), again.get(&b)), (Some(&sid_a), Some(&sid_b)));
    // And the first, asked for A again, keeps its SID.
    assert_eq!(first.sid_for(&a).unwrap(), sid_a);
}

#[test]
fn a_held_lock_makes_sid_for_give_up_after_its_wait() {
    let temp = TempDir::new("matter-sids-held");
    let folder = temp.path().join(SANDBOX_DIR);
    write::make_sandbox_dir(&folder).unwrap();
    let _held = write::lock_exclusive(&folder.join(MATTERS_LOCK)).unwrap();
    let mut sids = MatterSids::load(temp.path())
        .unwrap()
        .with_lock_wait(Duration::from_millis(200));
    let Err(SetupError::Failed(why)) = sids.sid_for(&matter()) else {
        panic!("sid_for went ahead while the lock was held");
    };
    assert!(why.contains(MATTERS_LOCK), "{why}");
    assert!(!why.contains(&*folder.to_string_lossy()), "{why}");
    assert!(sids.is_empty());
}

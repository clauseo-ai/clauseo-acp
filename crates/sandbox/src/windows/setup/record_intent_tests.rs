//! The intent's values on a real key (Windows, elevated: HKLM; `record_intent.rs`): written
//! `OwnerSid` first, read back as a partial record until the whole one is written, then cleared,
//! and gone with the set-up's values.

use std::path::PathBuf;

use super::intent::Intent;
use super::record_intent::{clear_intent, read_intent, write_intent};
use super::record_values::OWNER_SID;
use super::record_write::{self, read_texts};
use crate::windows::pool::MachineRecord;
use crate::windows::record::current_user_sid;
use crate::windows::test_acl::elevated_tests_enabled;

/// A key of this test's own, never the real record.
const KEY: &str = "SOFTWARE\\Clauseo\\SandboxTestIntent";

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
fn elevated_an_intent_is_partial_until_recorded_and_goes_with_the_record() {
    if !elevated_tests_enabled() {
        return;
    }
    drop(record_write::delete(KEY));
    let me = current_user_sid().unwrap();
    assert_eq!(read_intent(KEY).unwrap(), Intent::default());
    // Before the group and the first account: the owner, then what is pending.
    let users = vec!["ClauseoSb01".to_owned(), "NotAPoolName".to_owned()];
    write_intent(KEY, &me, true, &users).unwrap();
    let intent = read_intent(KEY).unwrap();
    assert_eq!(intent.owner.as_ref(), Some(&me));
    assert!(intent.group && intent.partial);
    assert_eq!(
        intent.users,
        ["ClauseoSb01"],
        "only pool names are read back"
    );
    assert_eq!(
        record_write::read(KEY).unwrap(),
        None,
        "partial reads as none"
    );
    // The whole record, then the intent cleared: nothing pending, nothing partial.
    let record = MachineRecord {
        owner: me.clone(),
        group: me.clone(),
        pool: vec![],
        data_dir: PathBuf::from(r"C:\ClauseoIntent\data"),
        removing: false,
    };
    record_write::write(KEY, &record).unwrap();
    assert!(!read_intent(KEY).unwrap().partial);
    write_intent(KEY, &me, false, &[]).unwrap();
    let intent = read_intent(KEY).unwrap();
    assert!(intent.is_empty(), "{intent:?}");
    assert_eq!(intent.owner.as_ref(), Some(&me));
    // Pending again, then the set-up's values go: the intent's with them, and the key.
    write_intent(KEY, &me, false, &users).unwrap();
    clear_intent(KEY).unwrap();
    assert!(read_intent(KEY).unwrap().is_empty());
    write_intent(KEY, &me, true, &users).unwrap();
    record_write::delete(KEY).unwrap();
    assert_eq!(read_intent(KEY).unwrap(), Intent::default());
    let left = read_texts(KEY, &[OWNER_SID]).unwrap();
    assert_eq!(left, [None]);
}

//! The record's admin-copy values on a real key (Windows, elevated: HKLM). A rollback or a
//! removal of the set-up (`record_write::delete`) keeps them; `--remove`'s last step deletes
//! them, and the key with them once it is empty.

use std::path::PathBuf;

use super::record_values::ADMIN_VALUES;
use super::record_values::{AdminValues, CopyOwner};
use super::record_write::{self, delete_admin_values, read_admin, read_texts};
use crate::windows::pool::MachineRecord;
use crate::windows::record::current_user_sid;
use crate::windows::test_acl::elevated_tests_enabled;

/// A key of this test's own, never the real record.
const KEY: &str = "SOFTWARE\\Clauseo\\SandboxTestAdminCopy";

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
fn elevated_a_rollback_keeps_the_admin_copy_values_and_remove_takes_them() {
    if !elevated_tests_enabled() {
        return;
    }
    drop(record_write::delete(KEY));
    drop(delete_admin_values(KEY));
    let me = current_user_sid().unwrap();
    let values = AdminValues {
        sha256: HASH.to_owned(),
        install_dir: PathBuf::from(r"C:\Users\l\AppData\Local\Clauseo"),
        version: "1.2.3".to_owned(),
        owner: CopyOwner::User(me.clone()),
    };
    record_write::write_admin_values(KEY, &values).unwrap();
    let record = MachineRecord {
        owner: me.clone(),
        group: me.clone(),
        pool: vec![],
        data_dir: PathBuf::from(r"C:\ClauseoAdminCopy\data"),
        removing: false,
    };
    record_write::write(KEY, &record).unwrap();
    // The rollback's (and the removal's) delete: the set-up's values go, --adopt's stay.
    record_write::delete(KEY).unwrap();
    assert_eq!(record_write::read(KEY).unwrap(), None);
    let texts = read_texts(KEY, &ADMIN_VALUES).unwrap();
    assert_eq!(
        texts,
        [
            Some(HASH.to_owned()),
            Some(r"C:\Users\l\AppData\Local\Clauseo".to_owned()),
            Some("1.2.3".to_owned()),
            Some(me.as_str().to_owned()),
        ]
    );
    assert_eq!(
        read_admin(KEY).unwrap(),
        (Some(CopyOwner::User(me)), Some("1.2.3".to_owned()))
    );
    // --remove's last step: the values, then the key, empty.
    delete_admin_values(KEY).unwrap();
    assert_eq!(
        read_texts(KEY, &ADMIN_VALUES).unwrap(),
        [None, None, None, None]
    );
    assert!(record_write::key_sddl(KEY).is_err(), "the key is gone");
}

//! The vendor key on the real `HKLM` (Windows, elevated): a write to a key of the test's own
//! secures the vendor key before it makes that key, and the vendor key keeps its pinned security.

use std::path::PathBuf;

use super::{VENDOR_SDDL, dacl_of, trusted_owner};
use crate::windows::record::current_user_sid;
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::record_values::{AdminValues, CopyOwner};
use crate::windows::setup::record_write::{self, delete_admin_values, key_sddl};
use crate::windows::test_acl::elevated_tests_enabled;

/// A key of this test's own, under the vendor key.
const KEY: &str = "SOFTWARE\\Clauseo\\SandboxTestVendor";

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
fn elevated_a_write_secures_the_vendor_key_before_its_key() {
    if !elevated_tests_enabled() {
        return;
    }
    drop(delete_admin_values(KEY));
    let values = AdminValues {
        sha256: HASH.to_owned(),
        install_dir: PathBuf::from(r"C:\ClauseoVendorTest"),
        version: "0.0.0".to_owned(),
        owner: CopyOwner::User(current_user_sid().unwrap()),
    };
    record_write::write_admin_values(KEY, &values).unwrap();
    let parent = key_sddl(VENDOR_KEY).unwrap();
    assert!(trusted_owner(&parent), "{parent}");
    assert_eq!(dacl_of(&parent), dacl_of(VENDOR_SDDL), "{parent}");
    drop(delete_admin_values(KEY));
}

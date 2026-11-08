//! The vendor key on the real registry (Windows, elevated), under a key of the test's own:
//! `SOFTWARE\ClauseoTest`, never the real vendor key. A write to a key secures the vendor key
//! before it makes that key, and a weak key with a planted subkey is repaired.

#![expect(unsafe_code, reason = "FFI to the registry's tree delete")]

use std::path::PathBuf;

use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RegDeleteTreeW};
use windows::core::HSTRING;

use super::os::Registry;
use super::{Report, Tree, VENDOR_SDDL, dacl_of, ensure, trusted_owner};
use crate::windows::acl::privilege::{Privileges, RECLAIM};
use crate::windows::record::current_user_sid;
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::record_values::{AdminValues, CopyOwner};
use crate::windows::setup::record_write::{self, create_key_with, delete_admin_values, key_sddl};
use crate::windows::test_acl::elevated_tests_enabled;

/// A key of this test's own, under the vendor key.
const KEY: &str = "SOFTWARE\\Clauseo\\SandboxTestVendor";

/// The test's own keys, all under this one, which [`Clear`] deletes.
const TEST_ROOT: &str = "SOFTWARE\\ClauseoTest";

/// A key of Users', which Users may make subkeys in: the weak parent a weaker run left.
const WEAK: &str = "O:BUD:P(A;CI;KA;;;BU)(A;CI;KA;;;BA)(A;CI;KA;;;SY)";

/// A subkey Users own, with Administrators let in so a plant can be made under it.
const PLANT: &str = "O:BUD:P(A;;KA;;;BU)(A;;KA;;;BA)";

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Deletes [`TEST_ROOT`] and everything under it, when made and when dropped.
struct Clear;

impl Clear {
    fn run() {
        let subkey = HSTRING::from(TEST_ROOT);
        // SAFETY: the subkey is a NUL-terminated HSTRING that lives for the call. A key that
        // isn't there is a failed call, which is the state wanted.
        let _deleted = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, &subkey) };
    }
}

impl Drop for Clear {
    fn drop(&mut self) {
        Clear::run();
    }
}

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

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
fn elevated_a_fresh_key_is_pinned_and_a_weak_one_with_a_planted_subkey_is_repaired() {
    if !elevated_tests_enabled() {
        return;
    }
    let _clear = Clear;
    Clear::run();
    // Owners other than the token's own need SeRestorePrivilege.
    let _held = Privileges::enable(&RECLAIM).unwrap();
    let path = format!("{TEST_ROOT}\\Vendor");

    // A key made fresh gets the pinned SD.
    let made = ensure(&mut Registry::at(&path), &path).unwrap();
    assert_eq!(made, Report::default());
    let fresh = key_sddl(&path).unwrap();
    assert!(trusted_owner(&fresh), "{fresh}");
    assert_eq!(dacl_of(&fresh), dacl_of(VENDOR_SDDL), "{fresh}");

    // A weaker run's key: owned by Users, Users may make subkeys, and a subkey Users own, with
    // one under it.
    let mut tree = Registry::at(&path);
    let root = tree.open_root().unwrap().unwrap();
    tree.set_sddl(&root, WEAK).unwrap();
    drop(root);
    create_key_with(&format!("{path}\\Planted"), PLANT).unwrap();
    create_key_with(&format!("{path}\\Planted\\Nested"), PLANT).unwrap();

    // Repaired: the key is pinned again, and the plant (with what is under it) is gone.
    let report = ensure(&mut Registry::at(&path), &path).unwrap();
    assert!(report.weaker_from.is_some(), "{report:?}");
    assert_eq!(report.removed, [format!("{path}\\Planted")]);
    let repaired = key_sddl(&path).unwrap();
    assert!(trusted_owner(&repaired), "{repaired}");
    assert_eq!(dacl_of(&repaired), dacl_of(VENDOR_SDDL), "{repaired}");
    assert!(
        key_sddl(&format!("{path}\\Planted")).is_err(),
        "the plant stays"
    );
    assert!(key_sddl(&format!("{path}\\Planted\\Nested")).is_err());
}

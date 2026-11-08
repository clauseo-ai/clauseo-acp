//! The vendor key on the real registry (Windows, elevated), under `SOFTWARE\ClauseoTest`, never the
//! real vendor key: a fresh root is pinned; a weak root is taken over with the subkeys under it
//! (a planted one, the installer-shaped key, a planted `Sandbox`, a real link child); a link
//! parent is refused.
#![expect(
    unsafe_code,
    reason = "FFI to the registry's tree delete and value calls"
)]

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_LINK,
    REG_OPTION_CREATE_LINK, REG_SZ, RegCreateKeyExW, RegDeleteKeyExW, RegDeleteTreeW,
    RegOpenKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

use super::os::{Key, Registry};
use super::security::Security;
use super::{Report, Tree, VENDOR_SDDL, VendorFault, ensure};
use crate::windows::acl::privilege::{Privileges, RECLAIM};
use crate::windows::setup::record_values::RECORD_SDDL;
use crate::windows::setup::record_write::{self, check, create_key_with, key_sddl, read_texts};
use crate::windows::test_acl::elevated_tests_enabled;

/// The test's own keys, all under this one, which [`Clear`] deletes.
const TEST_ROOT: &str = "SOFTWARE\\ClauseoTest";

/// A key of Users', which Users may write into: the weak root a weaker run left.
const WEAK: &str = "O:BUD:P(A;CI;KA;;;BU)(A;CI;KA;;;BA)(A;CI;KA;;;SY)";

/// A subkey Users own, with Administrators let in so a plant can be made under it.
const PLANT: &str = "O:BUD:P(A;;KA;;;BU)(A;;KA;;;BA)";

/// A key Administrators own, with Administrators let in: the installer's shape.
const ADMIN_KEY: &str = "O:BAD:P(A;CI;KA;;;BA)";

/// Deletes [`TEST_ROOT`] and everything under it, when cleared and when dropped.
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

/// A test key's path under `HKLM`, made into the NT path a link's target is written as.
fn nt_path(path: &str) -> String {
    format!("\\Registry\\Machine\\{path}")
}

/// Makes the link `path` (a registry link) pointing at `target`, an NT path.
fn make_link(path: &str, target: &str) -> Result<(), String> {
    let subkey = HSTRING::from(path);
    let mut key = HKEY::default();
    // SAFETY: the subkey is NUL-terminated and lives for the call; `key` is a live out-pointer,
    // owned by a `record_write::Key` below.
    let made = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_CREATE_LINK,
            KEY_ALL_ACCESS | KEY_WOW64_64KEY,
            None,
            &raw mut key,
            None,
        )
    };
    check("RegCreateKeyExW", made).map_err(|error| error.to_string())?;
    let key = record_write::Key(key);
    let name = HSTRING::from("SymbolicLinkValue");
    let data: Vec<u8> = target.encode_utf16().flat_map(u16::to_le_bytes).collect();
    // SAFETY: the key is open; the name is NUL-terminated; `data` is a live slice of the
    // target's UTF-16, as a link holds it.
    let set = unsafe { RegSetValueExW(key.0, &name, None, REG_LINK, Some(&data)) };
    check("RegSetValueExW", set).map_err(|error| error.to_string())?;
    Ok(())
}

/// Sets the `REG_SZ` value `name` of the key `path` to `text`.
fn set_string(path: &str, name: &str, text: &str) -> Result<(), String> {
    let subkey = HSTRING::from(path);
    let mut key = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `key` is a live out-pointer, owned below.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            &raw mut key,
        )
    };
    check("RegOpenKeyExW", opened).map_err(|error| error.to_string())?;
    let key = record_write::Key(key);
    let name = HSTRING::from(name);
    let data: Vec<u8> = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated; `data` is live.
    let set = unsafe { RegSetValueExW(key.0, &name, None, REG_SZ, Some(&data)) };
    check("RegSetValueExW", set).map_err(|error| error.to_string())?;
    Ok(())
}

/// Deletes the key `path` (one level, no subkeys), as the installer's uninstall does.
fn delete_key(path: &str) -> u32 {
    let subkey = HSTRING::from(path);
    // SAFETY: the subkey is NUL-terminated and lives for the call.
    let deleted = unsafe { RegDeleteKeyExW(HKEY_LOCAL_MACHINE, &subkey, KEY_WOW64_64KEY.0, None) };
    deleted.0
}

/// Whether the key `path` is there, following no link: a link is there while it is.
fn present(tree: &mut Registry, root: &Key, name: &str) -> Result<bool, String> {
    Ok(tree.open_child(root, name)?.is_some())
}

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
fn elevated_a_fresh_key_is_pinned_a_weak_one_is_taken_over_and_a_link_child_goes() {
    if !elevated_tests_enabled() {
        return;
    }
    let _clear = Clear;
    Clear::run();
    // Owners other than the token's own, and the take-over, need the privileges.
    let _held = Privileges::enable(&RECLAIM).unwrap();
    let path = format!("{TEST_ROOT}\\Vendor");
    let target = format!("{TEST_ROOT}\\Target");
    create_key_with(&target, "O:BAD:P(A;;KA;;;BA)").unwrap();

    // A key made fresh gets the pinned SD, with its effective DACL.
    let made = ensure(&mut Registry::at(&path), &path).unwrap();
    assert_eq!(made, Report::default());
    let fresh = Security::parse(&key_sddl(&path).unwrap());
    assert_eq!(fresh, Security::parse(VENDOR_SDDL));

    // A weaker run's root: owned by Users, Users may write into it.
    let mut tree = Registry::at(&path);
    let root = tree.take_over_root(WEAK).unwrap();
    drop(root);
    // Subkeys Users own: a plant, a plant under it, the installer's shape, and a planted Sandbox
    // with a value in it.
    create_key_with(&format!("{path}\\Planted"), PLANT).unwrap();
    create_key_with(&format!("{path}\\Planted\\Nested"), PLANT).unwrap();
    create_key_with(&format!("{path}\\Clauseo"), ADMIN_KEY).unwrap();
    create_key_with(&format!("{path}\\Sandbox"), PLANT).unwrap();
    set_string(&format!("{path}\\Sandbox"), "PoolSids", "planted").unwrap();
    // A real link under the root, pointing at Target.
    make_link(&format!("{path}\\LinkChild"), &nt_path(&target)).unwrap();

    let report = ensure(&mut Registry::at(&path), &path).unwrap();
    assert!(report.weaker_from.is_some(), "{report:?}");
    assert_eq!(report.removed, [format!("{path}\\LinkChild")]);
    assert_eq!(report.recreated, [format!("{path}\\Sandbox")]);
    assert!(report.taken_over.contains(&format!("{path}\\Planted")));
    assert!(
        report
            .taken_over
            .contains(&format!("{path}\\Planted\\Nested"))
    );
    assert!(report.taken_over.contains(&format!("{path}\\Clauseo")));

    let mut tree = Registry::at(&path);
    let root = tree.open_root().unwrap().unwrap();
    assert!(
        !present(&mut tree, &root, "LinkChild").unwrap(),
        "the link is gone"
    );
    assert!(key_sddl(&target).is_ok(), "the link's target is untouched");
    let repaired = Security::parse(&key_sddl(&path).unwrap());
    assert_eq!(repaired, Security::parse(VENDOR_SDDL));
    // The taken-over subkeys inherit the root's DACL: SYSTEM and Administrators full, Users read.
    for kept in ["Planted", "Planted\\Nested", "Clauseo"] {
        let security = Security::parse(&key_sddl(&format!("{path}\\{kept}")).unwrap());
        assert!(
            security.trusted_owner(),
            "{kept}: owned by SYSTEM or Administrators"
        );
        assert!(security.grants("SY", "KA"), "{kept}: SYSTEM full");
        assert!(security.grants("BA", "KA"), "{kept}: Administrators full");
        assert!(security.grants("BU", "KR"), "{kept}: Users read");
        assert!(!security.grants("BU", "KA"), "{kept}: no Users write");
    }
    // The planted Sandbox is gone and made again empty with its own SD, its value gone.
    let sandbox = Security::parse(&key_sddl(&format!("{path}\\Sandbox")).unwrap());
    assert_eq!(sandbox, Security::parse(&format!("O:BA{RECORD_SDDL}")));
    assert_eq!(
        read_texts(&format!("{path}\\Sandbox"), &["PoolSids"]).unwrap(),
        [None]
    );
}

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
fn elevated_an_installer_shaped_key_still_reads_writes_and_deletes_after_a_takeover() {
    if !elevated_tests_enabled() {
        return;
    }
    let _clear = Clear;
    Clear::run();
    let _held = Privileges::enable(&RECLAIM).unwrap();
    let path = format!("{TEST_ROOT}\\Vendor\\Clauseo");
    create_key_with(&path, ADMIN_KEY).unwrap();
    let root = format!("{TEST_ROOT}\\Vendor");
    ensure(&mut Registry::at(&root), &root).unwrap();

    // The admin still reads, writes and deletes the installer's key after the take-over.
    assert!(key_sddl(&path).is_ok());
    set_string(&path, "InstallDir", "C:\\Clauseo").unwrap();
    assert_eq!(
        read_texts(&path, &["InstallDir"]).unwrap(),
        [Some("C:\\Clauseo".to_owned())]
    );
    let security = Security::parse(&key_sddl(&path).unwrap());
    assert!(
        security.grants("BA", "KA"),
        "Administrators keep full control"
    );
    assert_eq!(
        delete_key(&path),
        ERROR_SUCCESS.0,
        "the installer's key deletes"
    );
    assert!(key_sddl(&path).is_err(), "and is gone");
}

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
fn elevated_a_link_parent_is_refused_before_anything_is_written() {
    if !elevated_tests_enabled() {
        return;
    }
    let _clear = Clear;
    Clear::run();
    let _held = Privileges::enable(&RECLAIM).unwrap();
    let target = format!("{TEST_ROOT}\\Target");
    create_key_with(&target, "O:BAD:P(A;;KA;;;BA)").unwrap();
    let parent = format!("{TEST_ROOT}\\LinkParent");
    make_link(&parent, &nt_path(&target)).unwrap();
    let before = key_sddl(&target).unwrap();
    assert_eq!(
        ensure(&mut Registry::at(&parent), &parent),
        Err(VendorFault::Link)
    );
    assert_eq!(
        key_sddl(&target).unwrap(),
        before,
        "the target is untouched"
    );
}

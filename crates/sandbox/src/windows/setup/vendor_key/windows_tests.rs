//! The vendor key on the real registry (Windows, elevated), under `SOFTWARE\ClauseoTest`, never the
//! real vendor key: a fresh root is pinned; a weak root is taken over with the subkeys under it
//! (Users' plants, one shutting out Administrators and denying SYSTEM, one cutting down its
//! owner's rights with OWNER RIGHTS, one whose planter sets its owner back mid-take, an
//! admin-owned key denying both, the installer-shaped key, a planted `Sandbox`, a real link
//! child whose unprotected target stays byte for byte); a link parent is refused. The walk runs
//! with `SeBackupPrivilege` alone, as set-up's does. A crash before the record's rename recovers
//! with its values, a copy beside the record is deleted, and a planted weak copy is not adopted.
#![expect(
    unsafe_code,
    reason = "FFI to the registry's tree delete and value calls"
)]

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_LINK,
    REG_OPTION_CREATE_LINK, REG_SAM_FLAGS, REG_SZ, RegCreateKeyExW, RegDeleteKeyExW,
    RegDeleteTreeW, RegOpenKeyExW, RegSetKeySecurity, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

use super::os::{AFTER_OWNER_WRITE, FAIL_FLUSH, FAIL_RENAME, Key, Registry};
use super::security::Security;
use super::{RECORD_LOCKED_SDDL, Report, Tree, VENDOR_SDDL, VendorFault, ensure};
use crate::windows::acl::privilege::{Privileges, RECLAIM};
use crate::windows::setup::lock::Descriptor;
use crate::windows::setup::record_values::RECORD_SDDL;
use crate::windows::setup::record_write::{self, check, create_key_with, key_sddl, read_texts};
use crate::windows::test_acl::elevated_tests_enabled;

/// The test's own keys, all under this one, which [`Clear`] deletes.
const TEST_ROOT: &str = "SOFTWARE\\ClauseoTest";

/// A key of Users', which Users may write into: the weak root a weaker run left.
const WEAK: &str = "O:BUD:P(A;CI;KA;;;BU)(A;CI;KA;;;BA)(A;CI;KA;;;SY)";

/// A subkey Users own, with Administrators let in so a plant can be made under it.
const PLANT: &str = "O:BUD:P(A;;KA;;;BU)(A;;KA;;;BA)";

/// A plant's DACL with no ACE for Administrators and a DENY one for SYSTEM.
const SHUT: &str = "D:P(D;;KA;;;SY)(A;;KR;;;BU)";

/// A plant's DACL that cuts its owner down to read, whoever that becomes.
const OWNER_READS: &str = "D:P(A;;KR;;;OW)(A;;KA;;;BU)";

/// A DACL denying SYSTEM and Administrators everything.
const DENY_BOTH: &str = "D:P(D;;KA;;;SY)(D;;KA;;;BA)";

/// A key Administrators own, with Administrators let in: the installer's shape.
const ADMIN_KEY: &str = "O:BAD:P(A;CI;KA;;;BA)";

/// A link's target's own DACL, unprotected, so it would take any ACE propagated through the link.
const TARGET: &str = "O:BAD:(A;;KA;;;BA)";

/// What set-up's walk holds: `SeBackupPrivilege` alone; a take-over enables restore itself.
const WALK: [&str; 1] = ["SeBackupPrivilege"];

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

/// Replaces the DACL of the key `path` (which lets Administrators in) with the protected
/// `dacl`, keeping its owner.
fn set_dacl(path: &str, dacl: &str) -> Result<(), String> {
    /// `WRITE_DAC`.
    const WRITE_DAC: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0004_0000);
    let descriptor = Descriptor::from_sddl(dacl).map_err(|error| error.to_string())?;
    let subkey = HSTRING::from(path);
    let mut key = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `key` is a live out-pointer, owned below.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            WRITE_DAC | KEY_WOW64_64KEY,
            &raw mut key,
        )
    };
    check("RegOpenKeyExW", opened).map_err(|error| error.to_string())?;
    let key = record_write::Key(key);
    // SAFETY: the key is open with WRITE_DAC; the descriptor is valid for the call.
    let set = unsafe {
        RegSetKeySecurity(
            key.0,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        )
    };
    check("RegSetKeySecurity", set).map_err(|error| error.to_string())?;
    Ok(())
}

/// Deletes the key `path` (one level, no subkeys), as the installer's uninstall does.
fn delete_key(path: &str) -> u32 {
    let subkey = HSTRING::from(path);
    // SAFETY: the subkey is NUL-terminated and lives for the call.
    let deleted = unsafe { RegDeleteKeyExW(HKEY_LOCAL_MACHINE, &subkey, KEY_WOW64_64KEY.0, None) };
    deleted.0
}

/// The key `path` opened with `WRITE_OWNER` while its DACL still lets Administrators in, as a
/// planter's handle opened before the take-over.
fn owner_handle(path: &str) -> Result<record_write::Key, String> {
    /// `WRITE_OWNER`.
    const WRITE_OWNER: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0008_0000);
    let subkey = HSTRING::from(path);
    let mut key = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `key` is a live out-pointer, owned below.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            WRITE_OWNER | KEY_WOW64_64KEY,
            &raw mut key,
        )
    };
    check("RegOpenKeyExW", opened).map_err(|error| error.to_string())?;
    Ok(record_write::Key(key))
}

/// The key `path` opened with `KEY_SET_VALUE` alone, as a planter's handle opened while its
/// DACL let Users write.
fn value_handle(path: &str) -> Result<record_write::Key, String> {
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
    Ok(record_write::Key(key))
}

/// Writes the `REG_SZ` value `name` through `key`, and returns the call's code.
fn write_through(key: &record_write::Key, name: &str, text: &str) -> u32 {
    let name = HSTRING::from(name);
    let data: Vec<u8> = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    // SAFETY: the handle is open (its key may be deleted); the name is NUL-terminated; `data`
    // is live.
    unsafe { RegSetValueExW(key.0, &name, None, REG_SZ, Some(&data)) }.0
}

/// Sets the owner of the key `key` is open on to Users, through that handle.
fn set_owner_users(key: HKEY) -> Result<(), String> {
    let _restore = Privileges::enable(&["SeRestorePrivilege"]).map_err(|e| e.to_string())?;
    let descriptor = Descriptor::from_sddl("O:BU").map_err(|error| error.to_string())?;
    // SAFETY: the key is open with WRITE_OWNER; the descriptor is valid for the call.
    let set = unsafe { RegSetKeySecurity(key, OWNER_SECURITY_INFORMATION, descriptor.0) };
    check("RegSetKeySecurity", set).map_err(|error| error.to_string())
}

/// Whether the key `name` under `root` is there, following no link.
fn present(root: &Key, name: &str) -> bool {
    Registry::open_child(root, name).is_ok()
}

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
#[expect(
    clippy::too_many_lines,
    reason = "one walk and the next, over the same planted keys"
)]
fn elevated_a_fresh_key_is_pinned_a_weak_one_is_taken_over_and_a_link_child_goes() {
    if !elevated_tests_enabled() {
        return;
    }
    let _clear = Clear;
    Clear::run();
    let path = format!("{TEST_ROOT}\\Vendor");
    let target = format!("{TEST_ROOT}\\Target");
    create_key_with(&target, TARGET).unwrap();

    // A key made fresh gets the pinned SD, with its effective DACL.
    let backup = Privileges::enable(&WALK).unwrap();
    let made = ensure(&mut Registry::at(&path), &path).unwrap();
    drop(backup);
    // Making the plants with owners other than the token's own needs the privileges.
    let held = Privileges::enable(&RECLAIM).unwrap();
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
    // A plant that shuts Administrators out and denies SYSTEM, and an admin-owned key that
    // denies both.
    create_key_with(&format!("{path}\\Shut"), PLANT).unwrap();
    set_dacl(&format!("{path}\\Shut"), SHUT).unwrap();
    create_key_with(&format!("{path}\\Denied"), ADMIN_KEY).unwrap();
    set_dacl(&format!("{path}\\Denied"), DENY_BOTH).unwrap();
    // A plant that cuts its owner's rights down, and one whose planter keeps a WRITE_OWNER
    // handle and sets the owner back after the take-over's first write.
    create_key_with(&format!("{path}\\Owned"), PLANT).unwrap();
    set_dacl(&format!("{path}\\Owned"), OWNER_READS).unwrap();
    create_key_with(&format!("{path}\\Racer"), PLANT).unwrap();
    let racer = owner_handle(&format!("{path}\\Racer")).unwrap();
    let raw = racer.0;
    let mut raced = false;
    AFTER_OWNER_WRITE.set(Some(Box::new(move |name: &str| {
        if name == "Racer" && !raced {
            raced = true;
            set_owner_users(raw).unwrap();
        }
    })));
    // A real link under the root, pointing at Target, whose SD is read as it was.
    make_link(&format!("{path}\\LinkChild"), &nt_path(&target)).unwrap();
    let target_before = key_sddl(&target).unwrap();
    drop(held);

    let backup = Privileges::enable(&WALK).unwrap();
    let report = ensure(&mut Registry::at(&path), &path).unwrap();
    drop(backup);
    AFTER_OWNER_WRITE.set(None);
    drop(racer);
    assert!(report.weaker_from.is_some(), "{report:?}");
    assert_eq!(report.removed, [format!("{path}\\LinkChild")]);
    assert_eq!(report.recreated, [format!("{path}\\Sandbox")]);
    assert_eq!(
        report.planted,
        [
            format!("{path}\\Owned"),
            format!("{path}\\Planted"),
            format!("{path}\\Racer"),
            format!("{path}\\Sandbox"),
            format!("{path}\\Shut")
        ]
    );
    assert_eq!(report.taken_over, [format!("{path}\\Denied")]);
    assert!(
        report.install_dir_reset.is_some(),
        "the installer's key is made again"
    );

    let mut tree = Registry::at(&path);
    let root = tree.open_root().unwrap().unwrap();
    for gone in ["LinkChild", "Owned", "Planted", "Racer", "Shut"] {
        assert!(!present(&root, gone), "{gone} is gone");
    }
    assert_eq!(
        key_sddl(&target).unwrap(),
        target_before,
        "the link's target is untouched, byte for byte"
    );
    let repaired = Security::parse(&key_sddl(&path).unwrap());
    assert_eq!(repaired, Security::parse(VENDOR_SDDL));
    // The taken-over subkeys inherit the root's DACL: SYSTEM and Administrators full, Users read.
    // Exactly those, and nothing else.
    for kept in ["Clauseo", "Denied"] {
        let security = Security::parse(&key_sddl(&format!("{path}\\{kept}")).unwrap());
        assert_eq!(security.owner, "S-1-5-18", "{kept}: owned by SYSTEM");
        assert!(!security.protected, "{kept}: inherits");
        assert!(security.is_vendor_set(), "{kept}: {security:?}");
    }
    // The planted Sandbox is gone and made again empty with its own SD, its value gone.
    let sandbox = Security::parse(&key_sddl(&format!("{path}\\Sandbox")).unwrap());
    assert_eq!(sandbox, Security::parse(&format!("O:BA{RECORD_SDDL}")));
    assert_eq!(
        read_texts(&format!("{path}\\Sandbox"), &["PoolSids"]).unwrap(),
        [None]
    );

    // The next walk, with a handle on each of Sandbox and the installer's key opened
    // while its DACL let Users write, then the DACL tightened to admins. Both keys are deleted all
    // the same, so the old handles write nowhere; Sandbox keeps its values, and the installer's key
    // gets the uninstall key's folder.
    let record = format!("{path}\\Sandbox");
    let installer = format!("{path}\\Clauseo");
    set_string(&record, "PoolSids", "kept").unwrap();
    set_dacl(&record, "D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KA;;;BU)").unwrap();
    set_dacl(&installer, "D:P(A;;KA;;;BA)(A;;KA;;;BU)").unwrap();
    let held_record = value_handle(&record).unwrap();
    let held_installer = value_handle(&installer).unwrap();
    set_dacl(&record, RECORD_SDDL).unwrap();
    set_dacl(&installer, "D:P(A;;KA;;;SY)(A;;KA;;;BA)").unwrap();
    let backup = Privileges::enable(&WALK).unwrap();
    let again = ensure(&mut Registry::at(&path), &path).unwrap();
    drop(backup);
    assert_eq!(again.recreated, std::slice::from_ref(&record), "{again:?}");
    assert!(again.writable_pinned.is_empty(), "{again:?}");
    assert!(again.retaken.is_empty(), "{again:?}");
    assert!(again.install_dir_reset.is_some(), "{again:?}");
    for (held, key) in [(&held_record, &record), (&held_installer, &installer)] {
        let code = write_through(held, "Planted", "C:\\Evil");
        assert_ne!(code, ERROR_SUCCESS.0, "{key}: the old handle's key is gone");
        assert_eq!(read_texts(key, &["Planted"]).unwrap(), [None], "{key}");
    }
    drop((held_record, held_installer));
    assert_eq!(
        Security::parse(&key_sddl(&record).unwrap()),
        Security::parse(&format!("O:BA{RECORD_SDDL}"))
    );
    assert_eq!(
        read_texts(&record, &["PoolSids"]).unwrap(),
        [Some("kept".to_owned())]
    );
    let remade = Security::parse(&key_sddl(&installer).unwrap());
    assert_eq!(
        remade.owner, "S-1-5-18",
        "the installer's key: owned by SYSTEM"
    );
    assert!(remade.is_vendor_set(), "{remade:?}");
}

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
fn elevated_an_installer_shaped_key_still_reads_writes_and_deletes_after_a_takeover() {
    if !elevated_tests_enabled() {
        return;
    }
    let _clear = Clear;
    Clear::run();
    let _held = Privileges::enable(&WALK).unwrap();
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
        security.is_vendor_set(),
        "SYSTEM and Administrators full, Users read, nothing else: {security:?}"
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
    let _held = Privileges::enable(&WALK).unwrap();
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

/// Writes a record's text values (owner, group, data folder) into the key `path`.
fn set_record(path: &str, owner: &str) -> Result<(), String> {
    set_string(path, "OwnerSid", owner)?;
    set_string(path, "GroupSid", "S-1-5-21-1-2-3-1100")?;
    set_string(path, "DataDir", "C:\\Users\\a\\Clauseo")
}

#[test]
#[ignore = "elevated: writes HKLM; CI's elevated step runs it"]
fn elevated_a_crash_mid_remake_recovers_the_record_and_a_weak_copy_is_deleted() {
    if !elevated_tests_enabled() {
        return;
    }
    let _clear = Clear;
    Clear::run();
    let path = format!("{TEST_ROOT}\\Vendor");
    let record = format!("{path}\\Sandbox");
    let copy = format!("{path}\\Sandbox.new");
    let pinned = format!("O:BA{RECORD_SDDL}");
    let names = ["OwnerSid", "GroupSid", "DataDir"];
    let owner = "S-1-5-21-1-2-3-1001";
    let held = Privileges::enable(&WALK).unwrap();
    ensure(&mut Registry::at(&path), &path).unwrap();
    create_key_with(&record, &pinned).unwrap();
    set_record(&record, owner).unwrap();
    let wanted = read_texts(&record, &names).unwrap();

    // A crash before the old record is deleted leaves it taken over with Users' read kept.
    FAIL_FLUSH.set(true);
    let crashed = ensure(&mut Registry::at(&path), &path);
    FAIL_FLUSH.set(false);
    assert!(crashed.is_err(), "{crashed:?}");
    let locked = Security::parse(&key_sddl(&record).unwrap());
    assert_eq!(locked, Security::parse(RECORD_LOCKED_SDDL));
    assert!(
        !locked.others_may_write(),
        "no one but SYSTEM and Administrators writes"
    );
    let users_read = locked
        .aces
        .iter()
        .any(|ace| ace.kind == "A" && ace.rights == "KR" && ace.sid == "S-1-5-32-545");
    assert!(
        users_read && locked.aces.iter().all(|ace| ace.kind == "A"),
        "{locked:?}"
    );
    assert_eq!(read_texts(&record, &names).unwrap(), wanted);
    let report = ensure(&mut Registry::at(&path), &path).unwrap();
    assert_eq!(report.dropped_copies, std::slice::from_ref(&copy));
    assert_eq!(read_texts(&record, &names).unwrap(), wanted);

    // A crash before the rename: the old record deleted, the copy left whole.
    FAIL_RENAME.set(true);
    let crashed = ensure(&mut Registry::at(&path), &path);
    FAIL_RENAME.set(false);
    assert!(crashed.is_err(), "{crashed:?}");
    assert!(key_sddl(&record).is_err() && key_sddl(&copy).is_ok());
    let report = ensure(&mut Registry::at(&path), &path).unwrap();
    assert_eq!(report.recovered, std::slice::from_ref(&copy));
    assert_eq!(read_texts(&record, &names).unwrap(), wanted);
    let security = Security::parse(&key_sddl(&record).unwrap());
    assert_eq!(security, Security::parse(&pinned));
    assert!(key_sddl(&copy).is_err(), "the copy is the record now");

    // A copy beside the record, as a crash before the delete leaves: deleted, the record kept.
    create_key_with(&copy, &pinned).unwrap();
    set_record(&copy, "S-1-5-21-9-9-9-1001").unwrap();
    let report = ensure(&mut Registry::at(&path), &path).unwrap();
    assert_eq!(report.dropped_copies, std::slice::from_ref(&copy));
    assert_eq!(read_texts(&record, &names).unwrap(), wanted);
    assert!(key_sddl(&copy).is_err());

    // A copy Users planted, with a weak SD and no record: deleted, not adopted.
    assert_eq!(delete_key(&record), ERROR_SUCCESS.0);
    drop(held);
    let reclaim = Privileges::enable(&RECLAIM).unwrap();
    create_key_with(&copy, PLANT).unwrap();
    set_record(&copy, owner).unwrap();
    drop(reclaim);
    let _held = Privileges::enable(&WALK).unwrap();
    let report = ensure(&mut Registry::at(&path), &path).unwrap();
    assert_eq!(report.dropped_copies, std::slice::from_ref(&copy));
    assert_eq!(report.recovered, Vec::<String>::new());
    assert!(key_sddl(&copy).is_err() && key_sddl(&record).is_err());
}

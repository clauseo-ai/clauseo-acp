//! WSL's deny on a real key (Windows, elevated: HKLM), with no WSL needed: a throwaway `AppID`
//! `HKLM\SOFTWARE\Classes\AppID\{random}` holding the stock MSI `LaunchPermission` (120 bytes),
//! whose DACL lets Administrators read and delete it but not set a value, so only the
//! backup-restore write gets through; a test group; and a record key of the test's own. The
//! targets are that `AppID` alone (`Throwaway`); every value read and write, and the record's,
//! go through the real `WindowsWsl`. Apply, check, remove; the key's owner and DACL never
//! change; everything is deleted on drop, a failed assertion included.
#![expect(unsafe_code, reason = "FFI to delete the test's key")]

use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY, RegDeleteKeyExW};
use windows::core::HSTRING;

use super::accounts;
use super::record_write::{create_key_with, delete_wsl, key_sddl};
use crate::Blocked;
use crate::windows::setup::logon::tests::random_hex;
use crate::windows::sids::Sid;
use crate::windows::test_acl::elevated_tests_enabled;
use crate::windows::wsl::{
    self, AppId, ClassEntry, Found, LXSS_USER_SESSION, LaunchSd, Permission, WSL_SERVICE,
    WindowsWsl, WslError, WslRead, WslRecord, WslWrite,
};

/// The `AppID`'s descriptor: owned by Administrators, protected; SYSTEM full control,
/// Administrators read and delete, Users read. No one but SYSTEM sets a value through a plain
/// open.
const APP_SDDL: &str = "O:BAD:P(A;;KA;;;SY)(A;;KR;;;BA)(A;;SD;;;BA)(A;;KR;;;BU)";

/// The stock MSI value: `O:BAG:BAD:(A;;CCDCSW;;;AU)(A;;CCDCSW;;;PS)(A;;CCDCSW;;;SY)`.
fn stock() -> Vec<u8> {
    let mut out = vec![
        1, 0, 4, 0x80, 0x14, 0, 0, 0, 0x24, 0, 0, 0, 0, 0, 0, 0, 0x34, 0, 0, 0,
    ];
    let ba = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x20, 2, 0, 0];
    out.extend_from_slice(&ba);
    out.extend_from_slice(&ba);
    out.extend_from_slice(&[2, 0, 0x44, 0, 3, 0, 0, 0]);
    for rid in [11, 10, 18] {
        out.extend_from_slice(&[
            0, 0, 0x14, 0, 0xB, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 5, rid, 0, 0, 0,
        ]);
    }
    out
}

/// The real registry, with the test's `AppID` as the only target and `WSLService` taken as
/// present, so the check runs as on a machine with WSL2.
struct Throwaway {
    real: WindowsWsl,
    app_id: AppId,
    group: Sid,
}

impl WslRead for Throwaway {
    fn service_exists(&self, name: &str) -> Result<bool, WslError> {
        Ok(name == WSL_SERVICE)
    }
    fn class(&self, clsid: &str) -> Result<ClassEntry, WslError> {
        Ok(if clsid == LXSS_USER_SESSION {
            ClassEntry::AppId(self.app_id.clone())
        } else {
            ClassEntry::Missing
        })
    }
    fn service_app_ids(&self, _services: &[&str]) -> Result<Vec<AppId>, WslError> {
        Ok(Vec::new())
    }
    fn launch_permission(&self, app_id: &AppId) -> Result<Permission, WslError> {
        self.real.launch_permission(app_id)
    }
    fn packaged_class(&self) -> Result<bool, WslError> {
        Ok(false)
    }
    fn group(&self) -> Option<Sid> {
        Some(self.group.clone())
    }
}

impl WslWrite for Throwaway {
    fn default_launch_permission(&self) -> Result<Option<Vec<u8>>, WslError> {
        self.real.default_launch_permission()
    }
    fn write_launch_permission(&mut self, app_id: &AppId, bytes: &[u8]) -> Result<(), WslError> {
        self.real.write_launch_permission(app_id, bytes)
    }
    fn delete_launch_permission(&mut self, app_id: &AppId) -> Result<(), WslError> {
        self.real.delete_launch_permission(app_id)
    }
    fn recorded(&self) -> Result<WslRecord, WslError> {
        self.real.recorded()
    }
    fn record(&mut self, record: &WslRecord) -> Result<(), WslError> {
        self.real.record(record)
    }
    fn forget(&mut self) -> Result<(), WslError> {
        self.real.forget()
    }
}

/// What the test made, deleted on drop.
struct Made {
    app_key: Option<String>,
    record_key: Option<String>,
    group: Option<String>,
}

impl Drop for Made {
    fn drop(&mut self) {
        if let Some(key) = &self.app_key {
            let subkey = HSTRING::from(key.as_str());
            // SAFETY: the subkey is NUL-terminated; the descriptor grants Administrators DELETE.
            let code =
                unsafe { RegDeleteKeyExW(HKEY_LOCAL_MACHINE, &subkey, KEY_WOW64_64KEY.0, None) };
            if code.is_err() {
                eprintln!("the test's AppID key wasn't deleted ({})", code.0);
            }
        }
        if let Some(key) = &self.record_key
            && let Err(error) = delete_wsl(key)
        {
            eprintln!("the test's record key wasn't deleted: {error}");
        }
        if let Some(group) = &self.group
            && let Err(error) = accounts::delete_group(group)
        {
            eprintln!("the test group wasn't deleted: {error}");
        }
    }
}

#[test]
#[ignore = "elevated: writes HKLM and makes a group; CI's elevated step runs it"]
fn elevated_wsls_deny_is_applied_checked_and_removed_on_a_throwaway_appid() {
    if !elevated_tests_enabled() {
        return;
    }
    let guid = format!(
        "{{{}-{}-{}-{}-{}}}",
        random_hex(4),
        random_hex(2),
        random_hex(2),
        random_hex(2),
        random_hex(6)
    );
    let app_id = AppId::parse(&guid).unwrap();
    let app_key = format!(r"SOFTWARE\Classes\AppID\{app_id}");
    let run = random_hex(3);
    let record_key = format!(r"SOFTWARE\Clauseo\SandboxTestWsl{run}");
    let group_name = format!("ClauseoWsl{run}");
    let mut made = Made {
        app_key: None,
        record_key: None,
        group: None,
    };
    create_key_with(&app_key, APP_SDDL).unwrap();
    made.app_key = Some(app_key.clone());
    create_key_with(&record_key, "O:BAD:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)").unwrap();
    made.record_key = Some(record_key.clone());
    let group = accounts::create_group(&group_name, "Clauseo's WSL deny test.").unwrap();
    made.group = Some(group_name);
    let mut machine = Throwaway {
        real: WindowsWsl::writer(&record_key),
        app_id: app_id.clone(),
        group: group.clone(),
    };
    // The stock value, placed through the backup-restore write itself: a plain open can't.
    machine.write_launch_permission(&app_id, &stock()).unwrap();
    let sddl = key_sddl(&app_key).unwrap();
    eprintln!("the test AppID's key: {sddl}");
    let read = |machine: &Throwaway| match machine.launch_permission(&app_id).unwrap() {
        Permission::Bytes(bytes) => bytes,
        other @ (Permission::NoKey | Permission::Absent) => panic!("{other:?}"),
    };
    assert_eq!(read(&machine), stock());
    assert_eq!(wsl::check(&machine), Err(Blocked::WslDenyMissing));

    let applied = wsl::apply(&mut machine, &group).unwrap();
    assert_eq!((applied.targets, applied.written), (1, 1), "{applied:?}");
    let merged = read(&machine);
    assert_eq!(merged.len(), 120 + 36);
    let sd = LaunchSd::parse(&merged).unwrap();
    assert!(sd.denies_first(&group));
    assert_eq!(sd.without_denies(&group).0.to_bytes(), stock());
    assert_eq!(
        key_sddl(&app_key).unwrap(),
        sddl,
        "the key's owner and DACL stay"
    );
    assert_eq!(wsl::check(&machine), Ok(Found::Denied(1)));
    assert_eq!(
        machine.recorded().unwrap().app_ids,
        std::slice::from_ref(&app_id)
    );
    // Again: nothing to write.
    assert_eq!(wsl::apply(&mut machine, &group).unwrap().unchanged, 1);

    let removed = wsl::remove(&mut machine, &group).unwrap();
    assert_eq!(removed.cleaned, 1, "{removed:?}");
    assert_eq!(read(&machine), stock(), "the stock bytes exactly");
    assert_eq!(key_sddl(&app_key).unwrap(), sddl);
    assert_eq!(machine.recorded().unwrap(), WslRecord::default());
    assert_eq!(wsl::check(&machine), Err(Blocked::WslDenyMissing));
}

//! The profile reset on a real machine (`resets.rs`): what a released pool user's agent could
//! leave in its own profile and hive is gone once set-up deletes the profile and a logon makes
//! it again. The marker file goes in `AppData\Local\Temp\Low` and `AppData\LocalLow`, and the
//! value under `HKCU\Software\AppDataLow`: the Low-labelled places a Low token can write.
//!
//! `elevated_` and `#[ignore]`: it runs only from CI's elevated step, behind
//! [`elevated_tests_enabled`]. Its accounts are `ClauseoSbTR` and six hex digits
//! ([`RESET_TEST_PREFIX`]), so its sweep never clears another family's.
#![expect(unsafe_code, reason = "FFI to the Win32 hive load and privilege calls")]

use std::path::Path;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, GetLastError, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_USERS, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, RegCreateKeyExW,
    RegLoadKeyW, RegOpenKeyExW, RegUnLoadKeyW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PCWSTR, w};

use super::super::super::profile::{delete_profile, profile_of};
use super::super::super::record_values::{RegValue, sz};
use super::super::super::record_write::{Key, check, set_value};
use super::super::log_on_once;
use super::{TestUser, clear_leftovers, me, random_hex};
use crate::windows::setup::strays::RESET_TEST_PREFIX;
use crate::windows::test_acl::elevated_tests_enabled;

/// The key the test writes under the user's hive, below `HKCU`.
const KEY: &str = r"Software\AppDataLow\ClauseoResetTest";
/// The marker file's name.
const MARKER: &str = "clauseo-reset-marker.txt";
/// How many times a hive load is tried while the profile service still holds the hive.
const LOAD_TRIES: u32 = 20;
/// The wait between two loads.
const LOAD_WAIT: Duration = Duration::from_millis(500);

/// A profile's hive, loaded at `HKU\<name>` and unloaded on drop. The keys opened under it must
/// be closed first.
struct Hive(String);

impl Hive {
    /// Loads `profile`'s `NTUSER.DAT` at `HKU\<name>`, tried again while the profile service
    /// still holds it after a logon.
    fn load(name: &str, profile: &Path) -> Hive {
        assert!(enable_restore_privilege(), "SeRestorePrivilege isn't held");
        let subkey = HSTRING::from(name);
        let file = HSTRING::from(profile.join("NTUSER.DAT").to_string_lossy().as_ref());
        let mut tries = 0;
        loop {
            tries += 1;
            // SAFETY: HKEY_USERS is a predefined key; both names are NUL-terminated and live for
            // the call.
            let loaded = unsafe { RegLoadKeyW(HKEY_USERS, &subkey, &file) };
            if loaded == ERROR_SUCCESS {
                return Hive(name.to_owned());
            }
            assert!(tries < LOAD_TRIES, "RegLoadKeyW {name}: {loaded:?}");
            wait();
        }
    }
}

impl Drop for Hive {
    fn drop(&mut self) {
        let subkey = HSTRING::from(self.0.as_str());
        // SAFETY: as in `load`; the name is NUL-terminated and lives for the call.
        let unloaded = unsafe { RegUnLoadKeyW(HKEY_USERS, &subkey) };
        if unloaded != ERROR_SUCCESS {
            eprintln!("HKU\\{} wasn't unloaded: {unloaded:?}", self.0);
        }
    }
}

/// Enables `SeRestorePrivilege` in this test process's token, which a hive load needs. `false`
/// when the token doesn't hold it.
fn enable_restore_privilege() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: a pseudo-handle and a live out-pointer; the handle is closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &raw mut token) }
        .is_err()
    {
        return false;
    }
    let mut luid = LUID::default();
    // SAFETY: a NUL-terminated literal and a live out-pointer.
    let found = unsafe { LookupPrivilegeValueW(None, w!("SeRestorePrivilege"), &raw mut luid) };
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    let mut enabled = false;
    if found.is_ok() {
        // SAFETY: the token was opened to adjust; `privileges` lives for the call.
        let adjusted = unsafe {
            AdjustTokenPrivileges(token, false, Some(&raw const privileges), 0, None, None)
        };
        // Success can still mean "not all assigned", which GetLastError tells.
        // SAFETY: reads this thread's last error, set by the call above.
        enabled = adjusted.is_ok() && unsafe { GetLastError() } == ERROR_SUCCESS;
    }
    // SAFETY: the token was opened above and is closed once, here.
    drop(unsafe { CloseHandle(token) });
    enabled
}

/// Whether `profile`'s hive holds `key`: `present` or `absent`.
fn hive_has(hive: &str, profile: &Path, key: &str) -> &'static str {
    let loaded = Hive::load(hive, profile);
    let path = HSTRING::from(format!("{}\\{key}", loaded.0));
    let mut opened = HKEY::default();
    // SAFETY: the path is NUL-terminated and lives for the call; `opened` is a live out-pointer,
    // closed by `Key` once found.
    let found = unsafe { RegOpenKeyExW(HKEY_USERS, &path, None, KEY_READ, &raw mut opened) };
    if found == ERROR_SUCCESS {
        drop(Key(opened));
        "present"
    } else {
        "absent"
    }
}

/// Sets `KEY`'s `Marker` value in `profile`'s hive to `left`, making the key as `reg add` did.
fn write_marker_value(hive: &str, profile: &Path) {
    let loaded = Hive::load(hive, profile);
    let path = HSTRING::from(format!("{}\\{KEY}", loaded.0));
    let mut opened = HKEY::default();
    // SAFETY: the path is NUL-terminated and lives for the call; `opened` is a live out-pointer,
    // closed by `Key`.
    let made = unsafe {
        RegCreateKeyExW(
            HKEY_USERS,
            &path,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &raw mut opened,
            None,
        )
    };
    check("RegCreateKeyExW", made).unwrap();
    let key = Key(opened);
    set_value(&key, "Marker", &RegValue::Sz(sz("left"))).unwrap();
}

/// Leaves the old agent's marker in `folder`, with its parents made (`New-Item -Force`).
#[expect(
    clippy::disallowed_methods,
    reason = "the test leaves a file in a user's profile, as the agent did"
)]
fn leave_marker(folder: &Path) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(folder.join(MARKER), "left").unwrap();
}

/// Waits [`LOAD_WAIT`] before a hive load is tried again.
#[expect(
    clippy::disallowed_methods,
    reason = "a bounded wait for the profile service to let go of a hive, with no clock to inject"
)]
fn wait() {
    std::thread::sleep(LOAD_WAIT);
}

#[test]
#[ignore = "elevated: run by CI's elevated step with CLAUSEO_ELEVATED_TESTS=1"]
fn elevated_a_profile_reset_drops_what_the_user_wrote_in_its_profile_and_hive() {
    if !elevated_tests_enabled() {
        return;
    }
    clear_leftovers(RESET_TEST_PREFIX);
    let user = TestUser::named(RESET_TEST_PREFIX, &random_hex(3));
    let hive = format!("ClauseoTR{}", random_hex(4));
    let first = log_on_once(&user.name, &user.password).unwrap();
    assert!(!first.temporary, "{first:?}");
    let temp_low = first.path.join(r"AppData\Local\Temp\Low");
    let local_low = first.path.join(r"AppData\LocalLow");
    // What a fresh profile has: printed for the VM notes, not asserted.
    eprintln!(
        "a fresh profile has Temp\\Low: {}; LocalLow: {}",
        temp_low.is_dir(),
        local_low.is_dir()
    );
    eprintln!(
        "a fresh hive has Software\\AppDataLow: {}",
        hive_has(&hive, &first.path, r"Software\AppDataLow")
    );

    // What the old matter's agent left.
    for folder in [&temp_low, &local_low] {
        leave_marker(folder);
        assert!(folder.join(MARKER).is_file(), "{}", folder.display());
    }
    write_marker_value(&hive, &first.path);
    assert_eq!(hive_has(&hive, &first.path, KEY), "present");

    // The reset: the profile deleted, then a logon makes it again, as set-up does.
    delete_profile(&user.sid, &me()).unwrap();
    assert!(!first.path.exists(), "{}", first.path.display());
    let again = log_on_once(&user.name, &user.password).unwrap();
    assert!(!again.temporary, "{again:?}");
    // The same folder back, as the set-up's reset requires (`resets::check_remade`).
    assert_eq!(
        again.path, first.path,
        "no <name>.<computer> beside a folder left behind"
    );
    // `ProfileList` may spell it in another case than `GetUserProfileDirectoryW`.
    let listed = profile_of(&user.sid).unwrap().unwrap().path;
    assert!(
        listed
            .to_string_lossy()
            .eq_ignore_ascii_case(&again.path.to_string_lossy()),
        "{} and {}",
        listed.display(),
        again.path.display()
    );
    assert!(
        again.path.join(r"AppData\LocalLow").is_dir(),
        "the profile is made again"
    );
    for folder in [r"AppData\Local\Temp\Low", r"AppData\LocalLow"] {
        let marker = again.path.join(folder).join(MARKER);
        assert!(!marker.exists(), "{}", marker.display());
    }
    assert_eq!(hive_has(&hive, &again.path, KEY), "absent");
    assert_eq!(
        hive_has(&hive, &again.path, r"Software\AppDataLow"),
        "present",
        "a new hive, made from Default"
    );
}

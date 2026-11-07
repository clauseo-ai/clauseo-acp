//! The profile reset on a real machine (`resets.rs`): what a released pool user's agent could
//! leave in its own profile and hive is gone once set-up deletes the profile and a logon makes
//! it again. The marker file goes in `AppData\Local\Temp\Low` and `AppData\LocalLow`, and the
//! value under `HKCU\Software\AppDataLow`: the Low-labelled places a Low token can write.
//!
//! `elevated_` and `#[ignore]`: it runs only from CI's elevated step, behind
//! [`elevated_tests_enabled`]. Its accounts are `ClauseoSbTR` and six hex digits
//! ([`RESET_TEST_PREFIX`]), so its sweep never clears another family's.

use std::path::Path;

use super::super::super::profile::{delete_profile, profile_of};
use super::super::log_on_once;
use super::{TestUser, clear_leftovers, me, powershell, random_hex};
use crate::windows::setup::strays::RESET_TEST_PREFIX;
use crate::windows::test_acl::elevated_tests_enabled;

/// The key the test writes under the user's hive, below `HKCU`.
const KEY: &str = r"Software\AppDataLow\ClauseoResetTest";
/// The marker file's name.
const MARKER: &str = "clauseo-reset-marker.txt";

/// A PowerShell script that loads `profile`'s hive at `HKU\<hive>` (tried again while the
/// profile service still holds it after a logon), runs `body` with `$h` the hive's root, and
/// unloads it.
fn with_hive(hive: &str, profile: &Path, body: &str) -> String {
    let file = profile.join("NTUSER.DAT");
    format!(
        "$h = 'HKU\\{hive}'; \
         for ($i = 0; $i -lt 20; $i++) {{ \
           & reg.exe load $h '{}' 2>$null | Out-Null; \
           if ($LASTEXITCODE -eq 0) {{ break }}; Start-Sleep -Milliseconds 500 }}; \
         if ($LASTEXITCODE -ne 0) {{ exit 1 }}; \
         try {{ {body} }} finally {{ [gc]::Collect(); & reg.exe unload $h 2>$null | Out-Null }}",
        file.display()
    )
}

/// Whether `profile`'s hive holds `key`: `present` or `absent`.
fn hive_has(hive: &str, profile: &Path, key: &str) -> String {
    powershell(&with_hive(
        hive,
        profile,
        &format!(
            "& reg.exe query \"$h\\{key}\" 2>$null | Out-Null; \
             if ($LASTEXITCODE -eq 0) {{ 'present' }} else {{ 'absent' }}"
        ),
    ))
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
        powershell(&format!(
            "New-Item -ItemType Directory -Force -Path '{0}' | Out-Null; \
             Set-Content -Path '{0}\\{MARKER}' -Value 'left'",
            folder.display()
        ));
        assert!(folder.join(MARKER).is_file(), "{}", folder.display());
    }
    powershell(&with_hive(
        &hive,
        &first.path,
        &format!(
            "& reg.exe add \"$h\\{KEY}\" /v Marker /t REG_SZ /d left /f | Out-Null; \
             if ($LASTEXITCODE -ne 0) {{ exit 1 }}"
        ),
    ));
    assert_eq!(hive_has(&hive, &first.path, KEY), "present");

    // The reset: the profile deleted, then a logon makes it again, as set-up does.
    delete_profile(&user.sid, &me()).unwrap();
    assert!(!first.path.exists(), "{}", first.path.display());
    let again = log_on_once(&user.name, &user.password).unwrap();
    assert!(!again.temporary, "{again:?}");
    assert_eq!(
        profile_of(&user.sid).unwrap().map(|p| p.path),
        Some(again.path.clone())
    );
    assert!(
        again.path.join(r"AppData\LocalLow").is_dir(),
        "the profile is made again"
    );
    for profile in [&first.path, &again.path] {
        for folder in [r"AppData\Local\Temp\Low", r"AppData\LocalLow"] {
            let marker = profile.join(folder).join(MARKER);
            assert!(!marker.exists(), "{}", marker.display());
        }
    }
    assert_eq!(hive_has(&hive, &again.path, KEY), "absent");
    assert_eq!(
        hive_has(&hive, &again.path, r"Software\AppDataLow"),
        "present",
        "a new hive, made from Default"
    );
}

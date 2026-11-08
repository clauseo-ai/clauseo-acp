//! The admin copy's rules on every OS, through the fake: each refusal, the order they come in,
//! and that the one path ever handed back is the admin copy's.

use std::path::{Path, PathBuf};

use super::fake::{Entry, Fake, HASH, descriptor, locked_dacl, program_files_dacl};
use super::*;

const USERS_SID: &str = "S-1-5-32-545";
const EVERYONE: &str = "S-1-1-0";
const LAWYER: &str = "S-1-5-21-1-2-3-1001";

fn exe() -> PathBuf {
    PathBuf::from(r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe")
}

fn sandbox() -> PathBuf {
    PathBuf::from(r"C:\Program Files\Clauseo\Sandbox")
}

/// Whether `checked` is the admin copy's path (`same_path`: off Windows `join` writes `/`).
fn passes(checked: Result<PathBuf, Blocked>) -> bool {
    checked.is_ok_and(|path| same_path(&path, &exe()))
}

/// The locked folder's entries plus `extra`.
fn with_extra(folder: bool, extra: Entry<'_>) -> Vec<u8> {
    let inherit = if folder { 0x3 } else { 0 };
    descriptor(
        ADMINISTRATORS,
        Some(&[
            (0, inherit, FULL_ACCESS, SYSTEM),
            (0, inherit, FULL_ACCESS, ADMINISTRATORS),
            (0, inherit, READ_EXECUTE, USERS_SID),
            extra,
        ]),
        true,
    )
}

#[test]
fn a_ready_copy_passes_and_the_path_is_the_admin_copy_alone() {
    let fake = Fake::ready();
    assert!(passes(check(&fake)));
    // Nothing outside Program Files but its volume root is ever looked at: the copy beside the
    // app is never built.
    let asked = fake.asked.borrow();
    assert!(!asked.is_empty());
    for path in asked.iter() {
        assert!(
            path.starts_with(r"C:\Program Files") || same_path(path, Path::new(r"C:\")),
            "asked about {}",
            path.display()
        );
        assert!(
            !path.to_string_lossy().contains("AppData"),
            "{}",
            path.display()
        );
    }
}

#[test]
fn the_admin_paths_are_under_program_files() {
    let pf = Path::new(r"C:\Program Files");
    assert!(same_path(&admin_dir(pf), &sandbox()));
    assert!(same_path(&admin_exe(pf), &exe()));
}

#[test]
fn a_copy_that_is_not_a_regular_file_is_missing() {
    for found in [Found::Missing, Found::Link, Found::Folder] {
        let mut fake = Fake::ready();
        fake.set(&exe(), found, locked_dacl(false));
        assert_eq!(check(&fake), Err(Blocked::AdminCopyMissing), "{found:?}");
    }
}

#[test]
fn no_recorded_hash_is_missing() {
    let mut fake = Fake::ready();
    fake.recorded = None;
    assert_eq!(check(&fake), Err(Blocked::AdminCopyMissing));
}

#[test]
fn another_hash_or_a_malformed_record_is_changed() {
    let mut fake = Fake::ready();
    fake.hash = HASH.replace('0', "f");
    assert_eq!(check(&fake), Err(Blocked::AdminCopyChanged));
    let short = HASH.get(1..).unwrap();
    for recorded in [&HASH.to_uppercase(), short, "not a hash"] {
        let mut fake = Fake::ready();
        fake.recorded = Some(recorded.to_owned());
        assert_eq!(check(&fake), Err(Blocked::AdminCopyChanged), "{recorded}");
    }
}

#[test]
fn a_planted_change_right_at_any_level_is_unsafe() {
    let levels = [
        (PathBuf::from(r"C:\Program Files"), Found::Folder),
        (PathBuf::from(r"C:\Program Files\Clauseo"), Found::Folder),
        (sandbox(), Found::Folder),
        (exe(), Found::File),
    ];
    let rights = [
        WRITE_ATTRIBUTES,
        WRITE_DATA,
        APPEND_DATA,
        DELETE_CHILD,
        DELETE,
        WRITE_DAC,
        WRITE_OWNER,
        GENERIC_WRITE,
        GENERIC_ALL,
        MAXIMUM_ALLOWED,
    ];
    for (path, found) in &levels {
        for right in rights {
            for sid in [USERS_SID, EVERYONE, LAWYER] {
                let mut fake = Fake::ready();
                let folder = *found == Found::Folder;
                fake.set(path, *found, with_extra(folder, (0, 0, right, sid)));
                assert_eq!(
                    check(&fake),
                    Err(Blocked::AdminCopyUnsafe),
                    "{} {right:#x} {sid}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn an_untrusted_owner_a_null_dacl_or_a_link_in_the_chain_is_unsafe() {
    let mut fake = Fake::ready();
    fake.set(
        &sandbox(),
        Found::Folder,
        descriptor(LAWYER, Some(&[]), true),
    );
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "owner");
    let mut fake = Fake::ready();
    fake.set(
        &sandbox(),
        Found::Folder,
        descriptor(ADMINISTRATORS, None, true),
    );
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "NULL DACL");
    let mut fake = Fake::ready();
    fake.set(&sandbox(), Found::Link, locked_dacl(true));
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "junction");
    let mut fake = Fake::ready();
    fake.paths.remove(r"c:\program files\clauseo");
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "a level gone");
}

#[test]
fn an_allow_whose_sid_cant_be_read_counts_as_another_accounts() {
    // An object entry (type 5): its SID sits after a GUID, so it isn't read; write is refused.
    let mut fake = Fake::ready();
    fake.set(
        &sandbox(),
        Found::Folder,
        with_extra(true, (5, 0, WRITE_DATA, USERS_SID)),
    );
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe));
}

#[test]
fn denies_inherit_only_entries_and_read_rights_are_safe() {
    let safe: [Entry<'_>; 5] = [
        (1, 0, FULL_ACCESS, USERS_SID),
        (0, 0x1B, GENERIC_ALL, "S-1-3-0"),
        (0, 0x0B, WRITE_DATA, USERS_SID),
        (0, 0, READ_EXECUTE, EVERYONE),
        (0, 0, FULL_ACCESS, TRUSTED_INSTALLER),
    ];
    for extra in safe {
        let mut fake = Fake::ready();
        fake.set(&sandbox(), Found::Folder, with_extra(true, extra));
        assert!(passes(check(&fake)), "{extra:?}");
    }
    assert!(level_safe(&program_files_dacl()));
}

#[test]
fn a_deny_doesnt_save_an_allow() {
    let mut fake = Fake::ready();
    let both = descriptor(
        ADMINISTRATORS,
        Some(&[(1, 0, WRITE_DATA, USERS_SID), (0, 0, WRITE_DATA, USERS_SID)]),
        true,
    );
    fake.set(&sandbox(), Found::Folder, both);
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe));
}

#[test]
fn an_unsafe_folder_is_named_before_a_changed_hash() {
    let mut fake = Fake::ready();
    fake.hash = HASH.replace('0', "f");
    fake.set(
        &sandbox(),
        Found::Folder,
        with_extra(true, (0, 0, WRITE_DATA, USERS_SID)),
    );
    assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe));
}

#[test]
fn locked_is_exactly_the_three_entries_protected_and_owned_by_administrators() {
    assert!(locked(&locked_dacl(true), true));
    assert!(locked(&locked_dacl(false), false));
    assert!(
        !locked(&locked_dacl(true), false),
        "folder entries on the file"
    );
    assert!(
        !locked(&locked_dacl(false), true),
        "file entries on the folder"
    );
    let open = descriptor(
        ADMINISTRATORS,
        Some(&[
            (0, 3, FULL_ACCESS, SYSTEM),
            (0, 3, FULL_ACCESS, ADMINISTRATORS),
            (0, 3, READ_EXECUTE, USERS_SID),
        ]),
        false,
    );
    assert!(!locked(&open, true), "not protected");
    assert!(
        !locked(&with_extra(true, (0, 3, READ_EXECUTE, EVERYONE)), true),
        "an extra entry"
    );
    let owned = descriptor(SYSTEM, Some(&[(0, 3, FULL_ACCESS, SYSTEM)]), true);
    assert!(!locked(&owned, true), "owner SYSTEM");
    let users_full = descriptor(
        ADMINISTRATORS,
        Some(&[
            (0, 3, FULL_ACCESS, SYSTEM),
            (0, 3, FULL_ACCESS, ADMINISTRATORS),
            (0, 3, FULL_ACCESS, USERS_SID),
        ]),
        true,
    );
    assert!(!locked(&users_full, true), "Users full control");
    assert!(
        !locked(&descriptor(ADMINISTRATORS, None, true), true),
        "NULL DACL"
    );
}

#[test]
fn paths_compare_as_windows_reads_them() {
    let a = Path::new(r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe");
    for same in [
        r"c:\program files\clauseo\sandbox\CLAUSEO-SANDBOX-SETUP.EXE",
        r"\\?\C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
        "C:/Program Files/Clauseo/Sandbox/clauseo-sandbox-setup.exe",
    ] {
        assert!(same_path(a, Path::new(same)), "{same}");
    }
    for other in [
        r"C:\Users\l\AppData\Local\Clauseo\clauseo-sandbox-setup.exe",
        r"C:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe.old",
        r"D:\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe",
    ] {
        assert!(!same_path(a, Path::new(other)), "{other}");
    }
}

#[test]
fn inside_is_strictly_below_part_by_part() {
    let profile = Path::new(r"C:\Users\lawyer");
    assert!(inside(
        Path::new(r"C:\Users\lawyer\AppData\Local\Clauseo"),
        profile
    ));
    assert!(inside(Path::new(r"c:\users\LAWYER\x"), profile));
    for outside in [
        r"C:\Users\lawyer",
        r"C:\Users\lawyer2\AppData",
        r"C:\Users\other\AppData",
        r"C:\Users\lawyer\..\other",
        r"D:\Users\lawyer\x",
    ] {
        assert!(!inside(Path::new(outside), profile), "{outside}");
    }
}

#[test]
fn hashes_are_64_lowercase_hex_digits() {
    assert_eq!(hex(&[0x00, 0xab, 0x7f]), "00ab7f");
    assert!(is_sha256_hex(HASH));
    assert!(!is_sha256_hex(&HASH.to_uppercase()));
    assert!(!is_sha256_hex(HASH.get(..63).unwrap()));
}

#[test]
fn anything_beside_the_copy_but_its_signature_staging_and_git_is_unsafe() {
    let mut fake = Fake::ready();
    for name in [
        "clauseo-sandbox-setup.exe.sig",
        "staging",
        "STAGING",
        "git",
        "Git",
    ] {
        fake.set(&sandbox().join(name), Found::File, locked_dacl(false));
    }
    assert!(passes(check(&fake)), "the expected entries");
    for stray in [
        "version.dll",
        "clauseo-sandbox-setup.exe.123.delete",
        "staging2",
        "git2",
        "_source",
    ] {
        let mut fake = Fake::ready();
        fake.set(&sandbox().join(stray), Found::File, locked_dacl(false));
        assert_eq!(check(&fake), Err(Blocked::AdminCopyUnsafe), "{stray}");
    }
}

#[test]
fn a_pool_another_user_owns_is_another_account_before_any_copy_check() {
    let mut fake = Fake::ready();
    fake.another = true;
    fake.recorded = None;
    assert_eq!(check_launchable(&fake), Err(Blocked::OtherAccount));
    assert!(fake.asked.borrow().is_empty(), "nothing else is read");
    let fake = Fake::ready();
    assert!(passes(check_launchable(&fake)));
}

#[test]
fn a_version_is_not_older_unless_it_sorts_below() {
    for (recorded, candidate) in [
        ("1.2.3", "1.2.3"),
        ("1.2.3", "1.2.4"),
        ("1.2.3", "1.10.0"),
        ("1.2.3", "v1.2.3"),
        ("1.2.3.0", "1.2.3"),
        ("1.2.3-beta.1", "1.2.3"),
        ("1.2.3-alpha", "1.2.3-alpha.1"),
        ("1.2.3-alpha.2", "1.2.3-alpha.10"),
        ("1.2.3-2", "1.2.3-alpha"),
        ("1.2.3+build.1", "1.2.3"),
        ("0.0.0", APP_VERSION),
    ] {
        assert!(
            version_not_older(recorded, candidate),
            "{recorded} -> {candidate}"
        );
    }
    for (recorded, candidate) in [
        ("1.2.4", "1.2.3"),
        ("2.0.0", "1.99.99"),
        ("1.2.3", "1.2.3-rc.1"),
        ("1.2.3-beta", "1.2.3-alpha"),
        ("1.2", "1.2.0"),
        ("1.2.3.4", "9.0.0"),
        ("garbage", "1.2.3"),
        ("1.2.3", ""),
        ("1.2.3.4.5", "9.0.0"),
        ("1.2.3-", "1.2.3"),
    ] {
        assert!(
            !version_not_older(recorded, candidate),
            "{recorded} -> {candidate}"
        );
    }
}

#[test]
fn adopt_records_the_version_in_the_form_the_refresh_writes() {
    // The refresh (PR 3) records the signed version, `X.Y.Z` or `X.Y.Z-pre` as release.yml
    // signs tauri.conf.json's `version`; --adopt records that same `version`, as it is.
    let parsed = parse_version(APP_VERSION).expect("tauri.conf.json's version is semver");
    assert_eq!(parsed.to_string(), APP_VERSION);
    assert!(parsed.build.is_empty(), "{APP_VERSION}");
    assert_eq!(
        compare_versions(APP_VERSION, APP_VERSION),
        Some(std::cmp::Ordering::Equal)
    );
}

#[test]
fn the_copy_goes_only_with_the_uninstall_of_the_install_that_placed_it() {
    let lawyer = crate::windows::sids::Sid::parse(LAWYER).unwrap();
    let other = crate::windows::sids::Sid::parse("S-1-5-21-1-2-3-1002").unwrap();
    let mine = CopyOwner::User(lawyer.clone());
    let machine = CopyOwner::Machine;
    let removal = |uninstall, per_machine, invoker| Removal {
        uninstall,
        per_machine,
        invoker,
    };
    assert!(copy_removable(
        &removal(true, true, None),
        false,
        Some(&machine)
    ));
    assert!(copy_removable(
        &removal(true, false, Some(&lawyer)),
        false,
        Some(&mine)
    ));
    // The app's plain --remove never takes it, whoever asks, whatever the record holds.
    for (per_machine, invoker, owner) in [
        (true, None, &machine),
        (false, Some(&lawyer), &mine),
        (false, Some(&other), &mine),
    ] {
        assert!(!copy_removable(
            &removal(false, per_machine, invoker),
            false,
            Some(owner)
        ));
    }
    // Another kind, another user, a pool still owned, or no owner recorded: it stays.
    assert!(!copy_removable(
        &removal(true, false, Some(&lawyer)),
        false,
        Some(&machine)
    ));
    assert!(!copy_removable(
        &removal(true, true, Some(&lawyer)),
        false,
        Some(&mine)
    ));
    assert!(!copy_removable(
        &removal(true, false, Some(&other)),
        false,
        Some(&mine)
    ));
    assert!(!copy_removable(
        &removal(true, false, None),
        false,
        Some(&mine)
    ));
    assert!(!copy_removable(
        &removal(true, true, None),
        true,
        Some(&machine)
    ));
    assert!(!copy_removable(&removal(true, true, None), false, None));
}

#[test]
fn the_state_names_the_copy_first_and_needed_only_with_a_verified_copy() {
    use crate::SetupState;
    let needed = || (SetupState::Needed, SetupState::Needed);
    // No copy, or no recorded hash: AdminCopyMissing, whatever the set-up says.
    let mut fake = Fake::ready();
    fake.paths.remove(&exe().to_string_lossy().to_lowercase());
    let (marker, combined) = needed();
    let missing = SetupState::Blocked(Blocked::AdminCopyMissing);
    assert_eq!(protection_state(&fake, marker, combined), missing);
    let mut fake = Fake::ready();
    fake.recorded = None;
    assert_eq!(
        protection_state(&fake, SetupState::Ready, SetupState::Ready),
        missing
    );
    // A bad hash: AdminCopyChanged.
    let mut fake = Fake::ready();
    fake.hash = HASH.replace('0', "f");
    let (marker, combined) = needed();
    assert_eq!(
        protection_state(&fake, marker, combined),
        SetupState::Blocked(Blocked::AdminCopyChanged)
    );
    // A verified copy and no set-up (after an in-app removal, say): Needed.
    let fake = Fake::ready();
    let (marker, combined) = needed();
    assert_eq!(
        protection_state(&fake, marker, combined),
        SetupState::Needed
    );
    assert_eq!(
        protection_state(&fake, SetupState::Ready, SetupState::Ready),
        SetupState::Ready
    );
    // A recorded copy cause the copy no longer has gives way; another cause stays.
    let stale = SetupState::Blocked(Blocked::AdminCopyMissing);
    assert_eq!(
        protection_state(&fake, SetupState::Needed, stale),
        SetupState::Needed
    );
    let logon = SetupState::Blocked(Blocked::LogonRight);
    assert_eq!(
        protection_state(&fake, SetupState::Needed, logon.clone()),
        logon
    );
}

/// After the app's plain `--remove`, the record keeps the admin copy's values and its owner but
/// no pool: the same lawyer's set-up passes the launcher's checks, another user's doesn't.
#[test]
fn after_an_in_app_removal_the_lawyers_set_up_passes_and_another_users_doesnt() {
    use crate::SetupState;
    let fake = Fake::ready();
    assert!(
        passes(check_launchable(&fake)),
        "the lawyer: the copy still verifies"
    );
    assert_eq!(
        protection_state(&fake, SetupState::Needed, SetupState::Needed),
        SetupState::Needed
    );
    let mut other = Fake::ready();
    other.another = true;
    assert_eq!(check_launchable(&other), Err(Blocked::OtherAccount));
    assert_eq!(
        protection_state(&other, SetupState::Needed, SetupState::Needed),
        SetupState::Blocked(Blocked::OtherAccount)
    );
}

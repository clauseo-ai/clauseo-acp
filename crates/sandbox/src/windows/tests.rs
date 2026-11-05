//! The folder check's answers on folders this test makes, and the admin shares on a Windows
//! runner.
//!
//! Past 500 lines: split tier B's record tests (from the registry read down) into a file of
//! their own when this grows.
#![expect(unsafe_code, reason = "FFI to read a test folder's DACL back as SDDL")]

use std::io;
use std::path::{Path, PathBuf};

use super::test_dacl;
use super::{FolderSupport, folder_support};
use crate::test_support::TempDir;

/// `OWNER RIGHTS` with Modify only, and no inherited entries: the owner loses the implicit
/// `WRITE_DAC` that an owner otherwise has. `0x1301bf` is Modify (`DELETE` included).
const OWNER_RIGHTS_MODIFY: &str = "D:P(A;OICI;0x1301bf;;;OW)";

/// `OWNER RIGHTS` with Full access: the owner keeps `WRITE_DAC`, granted by this ACE.
const OWNER_RIGHTS_FULL: &str = "D:P(A;OICI;FA;;;OW)";

#[test]
fn a_folder_the_user_made_is_ok() {
    let temp = TempDir::new("folder-ok");
    assert_eq!(folder_support(temp.path()).unwrap(), FolderSupport::Ok);
}

#[test]
fn a_folder_whose_owner_rights_exclude_write_dac_cant_change_permissions() {
    let temp = TempDir::new("folder-owner-rights");
    let folder = temp.dir("owner-rights");
    protect_with(&folder, OWNER_RIGHTS_MODIFY);
    assert_eq!(
        folder_support(&folder).unwrap(),
        FolderSupport::CantChangePermissions
    );
}

#[test]
fn a_folder_with_an_explicit_full_control_grant_is_ok() {
    // The owner's full access is granted by name: AccessCheck says yes when the DACL does.
    let temp = TempDir::new("folder-full-control");
    let folder = temp.dir("full-control");
    protect_with(&folder, OWNER_RIGHTS_FULL);
    assert_eq!(folder_support(&folder).unwrap(), FolderSupport::Ok);
}

#[test]
fn a_unc_path_is_network() {
    // The admin share `C$`: GitHub's Windows runners have it, so on CI a missing share fails the
    // test. Elsewhere (a developer's machine without it) the test says so and passes, as the
    // connectors' loopback test does. A subfolder, not the share root, which matters refuses.
    let share = Path::new(r"\\localhost\C$\Windows");
    if !share.is_dir() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI's Windows runners have the C$ admin share, and this run has none"
        );
        eprintln!("skipped: this machine has no \\\\localhost\\C$ share");
        return;
    }
    assert_eq!(folder_support(share).unwrap(), FolderSupport::Network);
}

#[test]
fn a_unc_path_that_cant_be_opened_is_still_network() {
    // The host doesn't resolve, so any open would fail; the answer comes before one.
    let share = Path::new(r"\\clauseo-no-such-host.invalid\matters\client");
    assert_eq!(folder_support(share).unwrap(), FolderSupport::Network);
}

#[test]
fn a_local_link_to_a_share_is_network() {
    // A symlink in a local folder that points at a share: the path itself looks local, so only
    // the final path (step 3) can tell. Making a symlink needs the privilege CI's admin runner
    // has; elsewhere the test says so and passes, but on CI it must run.
    let share = Path::new(r"\\localhost\C$\Windows");
    let temp = TempDir::new("folder-link-to-share");
    let link = if share.is_dir() {
        temp.symlink_dir("link", share).ok()
    } else {
        None
    };
    let Some(link) = link else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI's Windows runners have the C$ share and may make symlinks, and this run can't"
        );
        eprintln!("skipped: no C$ share here, or no right to make a symlink");
        return;
    };
    assert_eq!(folder_support(&link).unwrap(), FolderSupport::Network);
}

#[test]
fn a_lowercase_unc_prefix_is_network() {
    let share = Path::new(r"\\?\unc\clauseo-no-such-host.invalid\matters\client");
    assert_eq!(folder_support(share).unwrap(), FolderSupport::Network);
}

#[test]
fn a_unc_folder_under_a_parent_the_user_cant_list_is_network() {
    // The UNC path is answered before the folder is opened, so the parent's ACL never matters
    // here. The test pins that: it would fail if the open came first, and the admin token that
    // CI runs with can't change the answer. It doesn't reach the final-path branch (step 3),
    // which needs a link to a share.
    let temp = TempDir::new("folder-unc-unlistable");
    let outer = temp.dir("outer");
    let inner = temp.dir("outer/inner");
    let share_root = admin_share_root(temp.path());
    if !share_root.is_dir() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI's Windows runners have the admin shares, and this run has none"
        );
        eprintln!(
            "skipped: this machine has no {} share",
            share_root.display()
        );
        return;
    }
    protect_with(&inner, "D:P(A;OICI;FA;;;WD)");
    protect_with(&outer, "D:P(A;;0x1000a0;;;WD)"); // traverse, read attributes, synchronize
    let answer = folder_support(&admin_share_path(&inner));
    // Restored before the assert, so TempDir can list and remove `outer`.
    protect_with(&outer, "D:P(A;OICI;FA;;;WD)");
    assert_eq!(answer.unwrap(), FolderSupport::Network);
}

#[test]
fn a_device_unc_path_is_network() {
    let share = Path::new(r"\\.\UNC\clauseo-no-such-host.invalid\s\x");
    assert_eq!(folder_support(share).unwrap(), FolderSupport::Network);
}

#[test]
fn a_local_device_path_answers() {
    let temp = TempDir::new("folder-device-path");
    let device = PathBuf::from(format!(r"\\.\{}", temp.path().display()));
    assert_eq!(folder_support(&device).unwrap(), FolderSupport::Ok);
}

#[test]
fn a_missing_folder_is_not_found() {
    let temp = TempDir::new("folder-missing");
    let error = folder_support(&temp.path().join("missing")).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn a_file_is_not_a_folder() {
    let temp = TempDir::new("folder-file");
    let file = temp.file("note.txt", "a matter note");
    let error = folder_support(&file).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

/// The admin share that holds `path`'s drive, as a root: `C:\x` gives `\\localhost\C$\`.
fn admin_share_root(path: &Path) -> PathBuf {
    let (drive, _) = drive_and_rest(path);
    PathBuf::from(format!(r"\\localhost\{drive}$\"))
}

/// `path` as a UNC path through the admin share: `C:\x\y` gives `\\localhost\C$\x\y`.
pub(in crate::windows) fn admin_share_path(path: &Path) -> PathBuf {
    let (drive, rest) = drive_and_rest(path);
    PathBuf::from(format!(r"\\localhost\{drive}$\{rest}"))
}

/// `path`'s drive letter and the rest after `X:\`, for a path on a local drive.
fn drive_and_rest(path: &Path) -> (&str, &str) {
    let text = path.to_str().expect("a UTF-8 path on a local drive");
    text.split_once(":\\")
        .expect("a path that starts with a drive letter")
}

/// Gives `path` the protected DACL that `sddl` describes.
fn protect_with(path: &Path, sddl: &str) {
    test_dacl::protect_with(path, sddl).unwrap();
}

// Tier B's records: the registry read moved here from the desktop's check (#317), this user's
// SID, the OS facts and the set-up state on a runner that was never set up.

#[test]
fn the_registry_read_tells_missing_wrong_type_and_a_string_apart() {
    use windows::core::w;

    use super::RecordError;
    use super::record::read_string;

    let missing = read_string(w!("SOFTWARE\\Clauseo\\NoSuchKeyForTests"), w!("UserSid"));
    assert!(matches!(missing, Ok(None)), "{missing:?}");
    // Values every Windows has: ProductName is a REG_SZ, InstallDate a REG_DWORD.
    let version = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let name = read_string(version, w!("ProductName"));
    assert!(
        matches!(&name, Ok(Some(text)) if text.starts_with("Windows")),
        "{name:?}"
    );
    let missing_value = read_string(version, w!("NoSuchValueForTests"));
    assert!(matches!(missing_value, Ok(None)), "{missing_value:?}");
    let dword = read_string(version, w!("InstallDate"));
    assert!(matches!(dword, Err(RecordError::WrongType)), "{dword:?}");
    // REG_EXPAND_SZ is refused unexpanded: without RRF_NOEXPAND it would come back expanded, e.g.
    // `C:\Program Files`, and the environment (which the agent controls) would choose the text.
    let expand = read_string(
        w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion"),
        w!("ProgramFilesPath"),
    );
    assert!(matches!(expand, Err(RecordError::WrongType)), "{expand:?}");
    // The real record: a CI runner has none, a developer's machine may.
    let record = super::sandbox_user_sid();
    assert!(record.is_ok(), "{record:?}");
}

#[test]
fn this_users_sid_is_a_canonical_account_sid() {
    let user = super::record::current_user_sid().unwrap();
    assert!(user.as_str().starts_with("S-1-5-"), "{user}");
}

#[test]
fn the_runner_is_a_supported_windows() {
    let os = super::os_facts();
    assert!(!os.arm64, "{os:?}");
    let build = os.build.unwrap();
    assert!(build >= crate::detect::FIRST_BUILD, "{build}");
}

#[test]
fn a_data_folder_without_a_marker_needs_set_up() {
    let temp = TempDir::new("setup-state");
    assert_eq!(super::setup_state(temp.path()), crate::SetupState::Needed);
}

// The review's fixes: machine accounts never load as matter SIDs, and the sandbox folder's
// protected DACL.

/// A fixed matter id (a v7 UUID).
const MATTER_A: &str = "0190f5c8-7b1a-7c3d-8e4f-0123456789ab";

/// `matters.json` under `temp` mapping `MATTER_A` to `sid`.
fn matters_with(temp: &TempDir, sid: &str) {
    temp.file(
        "sandbox/matters.json",
        &format!(r#"{{"version":1,"matters":{{"{MATTER_A}":"{sid}"}}}}"#),
    );
}

#[test]
fn machine_accounts_never_load_as_matter_sids() {
    use super::matters::MatterSids;
    use super::sids::{Reserved, Sid};

    let temp = TempDir::new("matter-sids-accounts");
    let user = super::record::current_user_sid().unwrap();
    matters_with(&temp, user.as_str());
    assert!(MatterSids::load(temp.path()).is_err(), "{user}");
    if let Some(record) = super::sandbox_user_sid().unwrap() {
        // A machine that was set up: the real record.
        matters_with(&temp, &record);
        assert!(MatterSids::load(temp.path()).is_err(), "{record}");
    } else {
        // A CI runner has no record: the same reservation, given by hand.
        let sandbox = Sid::parse("S-1-5-21-11-22-33-1005").unwrap();
        matters_with(&temp, sandbox.as_str());
        let reserved = Reserved::new([user.clone(), sandbox]);
        assert!(MatterSids::load_with(temp.path(), reserved).is_err());
    }
    // New matter SIDs stay out of this user's domain.
    let fresh = TempDir::new("matter-sids-fresh");
    let mut sids = MatterSids::load(fresh.path()).unwrap();
    let matter: clauseo_protocol::MatterId = MATTER_A.parse().unwrap();
    let sid = sids.sid_for(&matter).unwrap();
    assert!(
        user.domain().is_none() || sid.domain() != user.domain(),
        "{sid}"
    );
}

/// The SDDL of `path`'s DACL.
fn dacl_sddl(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt as _;

    use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
        SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
    use windows::core::{PCWSTR, PWSTR};

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `wide` is NUL-terminated; `descriptor` is a live out-pointer, freed below.
    let status = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(wide.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            &raw mut descriptor,
        )
    };
    assert_eq!(status, ERROR_SUCCESS);
    let mut text = PWSTR::null();
    // SAFETY: the descriptor is valid; `text` is a live out-pointer, freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &raw mut text,
            None,
        )
    }
    .unwrap();
    // SAFETY: a NUL-terminated string from the call above.
    let sddl = unsafe { text.to_string() }.unwrap();
    // SAFETY: both were allocated with LocalAlloc and are freed once.
    unsafe {
        LocalFree(Some(HLOCAL(text.0.cast())));
        LocalFree(Some(HLOCAL(descriptor.0)));
    }
    sddl
}

/// Asserts `path`'s DACL is protected and holds exactly three entries: full access for the
/// user, `SYSTEM` and `Administrators`.
fn assert_private(path: &Path) {
    let user = super::record::current_user_sid().unwrap();
    let sddl = dacl_sddl(path);
    assert!(sddl.starts_with("D:P"), "{sddl}");
    let mut trustees: Vec<String> = sddl
        .split('(')
        .skip(1)
        .map(|ace| {
            let fields: Vec<&str> = ace.trim_end_matches(')').split(';').collect();
            assert_eq!(fields.len(), 6, "{sddl}");
            assert_eq!((fields[0], fields[2]), ("A", "FA"), "{sddl}");
            fields[5].to_owned()
        })
        .collect();
    trustees.sort();
    // The local Administrator account (RID 500) prints as the alias LA.
    let me = if user.as_str().ends_with("-500") {
        "LA".to_owned()
    } else {
        user.as_str().to_owned()
    };
    let mut expected = vec![me, "BA".to_owned(), "SY".to_owned()];
    expected.sort();
    assert_eq!(trustees, expected, "{sddl}");
}

#[test]
fn the_sandbox_folder_gets_a_protected_private_dacl_again_and_again() {
    let temp = TempDir::new("sandbox-dacl");
    let folder = temp.path().join("sandbox");
    super::write::make_sandbox_dir(&folder).unwrap();
    assert_private(&folder);
    // A DACL changed behind the folder's back is put back at the next make.
    test_dacl::protect_with(&folder, "D:P(A;OICI;FA;;;WD)").unwrap();
    super::write::make_sandbox_dir(&folder).unwrap();
    assert_private(&folder);
}

/// Enables `SeRestorePrivilege` in this test process's token, so it may give a folder to another
/// account. `false` when the token lacks it (a non-admin runner).
fn enable_restore_privilege() -> bool {
    use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, GetLastError, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::w;

    let mut token = HANDLE::default();
    // SAFETY: a pseudo-handle and a live out-pointer.
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
    // SAFETY: the token was opened to adjust; `privileges` lives for the call. Its success can
    // still mean "not all assigned", which GetLastError tells.
    let enabled = found.is_ok()
        && unsafe { AdjustTokenPrivileges(token, false, Some(&raw const privileges), 0, None, None) }
            .is_ok()
        // SAFETY: reads this thread's last error, set by the call above.
        && unsafe { GetLastError() } == ERROR_SUCCESS;
    // SAFETY: opened above, closed once.
    drop(unsafe { CloseHandle(token) });
    enabled
}

#[test]
fn a_sandbox_folder_owned_by_another_account_is_taken_back() {
    use super::private_dir::{owner, owner_allowed, set_owner};
    use super::sids::Sid;

    let user = super::record::current_user_sid().unwrap();
    let local_service = Sid::parse("S-1-5-19").unwrap();
    assert!(owner_allowed(&user, &user));
    assert!(owner_allowed(&Sid::parse("S-1-5-18").unwrap(), &user));
    assert!(owner_allowed(&Sid::parse("S-1-5-32-544").unwrap(), &user));
    assert!(!owner_allowed(&local_service, &user));

    let temp = TempDir::new("sandbox-owner");
    let folder = temp.path().join("sandbox");
    super::write::make_sandbox_dir(&folder).unwrap();
    if !enable_restore_privilege() {
        println!("skipped the re-own case: this token can't give a folder away");
        return;
    }
    set_owner(&folder, &local_service).unwrap();
    assert_eq!(owner(&folder).unwrap(), local_service);
    // The folder's DACL still grants the user full access, WRITE_OWNER included, so the user
    // takes it back.
    super::write::make_sandbox_dir(&folder).unwrap();
    assert_eq!(owner(&folder).unwrap(), user);
    assert_private(&folder);
}

/// A sandbox folder made by `make_sandbox_dir` under a fresh temp folder.
fn sandbox_folder(test: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new(test);
    let folder = temp.path().join("sandbox");
    super::write::make_sandbox_dir(&folder).unwrap();
    (temp, folder)
}

#[test]
fn record_files_get_the_folders_three_entries_as_their_own() {
    use super::matters::MatterSids;

    let (temp, folder) = sandbox_folder("record-file-ace");
    let file = temp.file("sandbox/matters.json", r#"{"version":1,"matters":{}}"#);
    test_dacl::protect_with(&file, "D:P(A;;FA;;;WD)").unwrap();
    assert!(dacl_sddl(&file).contains(";WD)"));
    let mut sids = MatterSids::load(temp.path()).unwrap();
    // Twice: the first save replaces matters.json with a new file, which the second checks.
    for matter in [MATTER_A, "0190f5c8-7b1a-7c3d-8e4f-0123456789ac"] {
        sids.sid_for(&matter.parse().unwrap()).unwrap();
        assert_private(&folder.join("matters.json"));
        assert_private(&folder.join("matters.lock"));
    }
    assert_eq!(MatterSids::load(temp.path()).unwrap().len(), 2);
    drop(file);
}

#[test]
fn a_record_file_owned_by_another_account_is_taken_back() {
    use super::private_dir::{owner, set_owner};
    use super::sids::Sid;

    let (temp, _folder) = sandbox_folder("record-file-owner");
    let file = temp.file("sandbox/matters.json", "{}");
    if !enable_restore_privilege() {
        println!("skipped: this token can't give a file away");
        return;
    }
    let local_service = Sid::parse("S-1-5-19").unwrap();
    set_owner(&file, &local_service).unwrap();
    assert_eq!(owner(&file).unwrap(), local_service);
    super::write::check_owned_file(&file).unwrap();
    assert_eq!(
        owner(&file).unwrap(),
        super::record::current_user_sid().unwrap()
    );
}

#[test]
fn a_hard_linked_record_file_is_refused() {
    let (temp, _folder) = sandbox_folder("record-file-hard-link");
    let outside = temp.file("outside.json", "{}");
    let linked = temp.hard_link("sandbox/matters.json", &outside);
    let error = super::write::check_owned_file(&linked).unwrap_err();
    assert!(error.to_string().contains("hard links"), "{error}");
}

#[test]
fn a_missing_record_file_is_fine() {
    let (_temp, folder) = sandbox_folder("record-file-missing");
    super::write::check_owned_file(&folder.join("matters.json")).unwrap();
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test makes its junction with cmd's mklink /J, which exits at once"
)]
fn a_junction_named_sandbox_is_refused() {
    let temp = TempDir::new("sandbox-junction");
    let elsewhere = temp.dir("elsewhere");
    let junction = temp.path().join("sandbox");
    let made = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&junction)
        .arg(&elsewhere)
        .status()
        .unwrap();
    assert!(made.success());
    let error = super::write::make_sandbox_dir(&junction).unwrap_err();
    assert!(error.to_string().contains("reparse point"), "{error}");
    // The folder the junction names kept its DACL: nothing was set through the link.
    assert!(!dacl_sddl(&elsewhere).starts_with("D:P"));
}

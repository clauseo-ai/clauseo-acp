//! The folder check's answers on folders this test makes, and the admin shares on a Windows
//! runner.

#![expect(
    unsafe_code,
    reason = "FFI to the security calls that give the test folder its protected DACL"
)]

use std::io;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetNamedSecurityInfoW,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};
use windows::core::{BOOL, PCWSTR};

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
    let link = temp.path().join("link");
    if !share.is_dir() || std::os::windows::fs::symlink_dir(share, &link).is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI's Windows runners have the C$ share and may make symlinks, and this run can't"
        );
        eprintln!("skipped: no C$ share here, or no right to make a symlink");
        return;
    }
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
fn admin_share_path(path: &Path) -> PathBuf {
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
    let text: Vec<u16> = sddl.encode_utf16().chain(once(0)).collect();
    let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
    // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` is a live out-pointer,
    // and the descriptor it receives is freed by `Local` below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(text.as_ptr()),
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .unwrap();
    let _descriptor = Local(descriptor);
    let mut present = BOOL(0);
    let mut dacl: *mut ACL = null_mut();
    let mut defaulted = BOOL(0);
    // SAFETY: the descriptor is valid until `_descriptor` drops; the out-pointers are live.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &raw mut present,
            &raw mut dacl,
            &raw mut defaulted,
        )
    }
    .unwrap();
    assert!(present.as_bool(), "{sddl} has no DACL");
    let target: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
    // SAFETY: `target` is NUL-terminated and outlives the call; the DACL points into the
    // descriptor, which lives until this function returns.
    let status = unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(target.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl.cast_const()),
            None,
        )
    };
    assert_eq!(status, ERROR_SUCCESS, "SetNamedSecurityInfoW on {path:?}");
}

/// A descriptor from `ConvertStringSecurityDescriptorToSecurityDescriptorW`, freed on drop.
struct Local(PSECURITY_DESCRIPTOR);

impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated with LocalAlloc and is freed here, once.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

//! The folder check's answers on folders this test makes, and the `C$` share on a Windows runner.

#![expect(
    unsafe_code,
    reason = "FFI to the security calls that give the test folder its protected DACL"
)]

use std::io;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
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

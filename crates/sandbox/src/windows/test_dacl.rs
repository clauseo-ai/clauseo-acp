//! Tests' folders with a DACL of their own: [`protect_with`] gives a folder the protected DACL an
//! SDDL string describes, as a folder the lawyer can't re-permission has. For this crate's tests
//! and, through the `test-support` feature, the host's.
#![expect(
    unsafe_code,
    reason = "FFI to the security calls that give a test folder its protected DACL"
)]

use std::io;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::ptr::null_mut;

use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree, WIN32_ERROR};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetNamedSecurityInfoW,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, OBJECT_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows::core::{BOOL, PCWSTR};

/// Gives `path` the protected DACL that `sddl` describes, such as
/// `D:P(A;OICI;0x1301bf;;;OW)`: no entry is inherited from the parent any more.
pub fn protect_with(path: &Path, sddl: &str) -> io::Result<()> {
    set_dacl(path, sddl, PROTECTED_DACL_SECURITY_INFORMATION)
}

/// Gives `path` the explicit entries `sddl` describes, with its parent's inheritable entries
/// after them, through `SetNamedSecurityInfoW`, which marks the DACL auto-inherited
/// (`SE_DACL_AUTO_INHERITED`): a folder in that state whatever its parent's state.
pub fn auto_inherit_with(path: &Path, sddl: &str) -> io::Result<()> {
    set_dacl(path, sddl, UNPROTECTED_DACL_SECURITY_INFORMATION)
}

/// `SetNamedSecurityInfoW` with the DACL of `sddl` and `DACL_SECURITY_INFORMATION | how`.
fn set_dacl(path: &Path, sddl: &str, how: OBJECT_SECURITY_INFORMATION) -> io::Result<()> {
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
    .map_err(io::Error::other)?;
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
    .map_err(io::Error::other)?;
    if !present.as_bool() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{sddl} has no DACL"),
        ));
    }
    let target: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
    // SAFETY: `target` is NUL-terminated and outlives the call; the DACL points into the
    // descriptor, which lives until this function returns.
    let status: WIN32_ERROR = unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(target.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | how,
            None,
            None,
            Some(dacl.cast_const()),
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::other(format!(
            "SetNamedSecurityInfoW failed with {}",
            status.0
        )));
    }
    Ok(())
}

/// A descriptor from `ConvertStringSecurityDescriptorToSecurityDescriptorW`, freed on drop.
struct Local(PSECURITY_DESCRIPTOR);

impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated with LocalAlloc and is freed here, once.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// Removes `SeBackupPrivilege`, `SeRestorePrivilege` and `SeTakeOwnershipPrivilege` from this
/// test process's token, so an elevated admin token opens files as the lawyer's Medium token
/// would: with them, a backup-intent open gets past a DACL that shuts the owner out. nextest
/// runs each test in its own process. A token without them is left as it is.
#[cfg(test)]
pub(crate) fn drop_privileges() {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_REMOVED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::w;

    let mut token = HANDLE::default();
    // SAFETY: a pseudo-handle and a live out-pointer; the handle is closed below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &raw mut token) }
        .unwrap();
    for name in [
        w!("SeBackupPrivilege"),
        w!("SeRestorePrivilege"),
        w!("SeTakeOwnershipPrivilege"),
    ] {
        let mut luid = LUID::default();
        // SAFETY: a NUL-terminated literal and a live out-pointer.
        unsafe { LookupPrivilegeValueW(None, name, &raw mut luid) }.unwrap();
        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_REMOVED,
            }],
        };
        // SAFETY: the token was opened to adjust; `privileges` lives for the call. A token
        // without the privilege succeeds with "not all assigned", which is what is wanted.
        unsafe { AdjustTokenPrivileges(token, false, Some(&raw const privileges), 0, None, None) }
            .unwrap();
    }
    // SAFETY: the token was opened above and is closed once, here.
    unsafe { CloseHandle(token) }.unwrap();
}

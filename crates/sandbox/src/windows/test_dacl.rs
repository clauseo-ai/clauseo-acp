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
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};
use windows::core::{BOOL, PCWSTR};

/// Gives `path` the protected DACL that `sddl` describes, such as
/// `D:P(A;OICI;0x1301bf;;;OW)`: no entry is inherited from the parent any more.
pub fn protect_with(path: &Path, sddl: &str) -> io::Result<()> {
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
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
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

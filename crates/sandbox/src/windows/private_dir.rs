//! The sandbox folder's protected DACL: `<data_dir>\sandbox\` holds the matter SIDs, the set-up
//! marker and (S15 PR4) the sandbox user's sealed password, so only the desktop user, `SYSTEM`
//! and `Administrators` may open it, with nothing inherited from the data folder.
#![expect(
    unsafe_code,
    reason = "FFI to the security calls that set the folder's DACL"
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

use super::record::current_user_sid;

/// The DACL for the desktop user `user`: full access, inherited by files and folders inside,
/// for `user`, `SYSTEM` and `Administrators`; `P` protects it from the parent's entries.
pub(crate) fn sddl(user: &str) -> String {
    format!("D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")
}

/// Gives the folder `path` the protected DACL of [`sddl`] for this process's user. Set every
/// time, which also puts it back if it was changed.
pub(crate) fn protect(path: &Path) -> io::Result<()> {
    let user = current_user_sid()?;
    let text: Vec<u16> = sddl(user.as_str()).encode_utf16().chain(once(0)).collect();
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
    if !present.as_bool() || dacl.is_null() {
        return Err(io::Error::other("the sandbox folder's SDDL gave no DACL"));
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
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status.0).unwrap_or(i32::MAX),
        ));
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

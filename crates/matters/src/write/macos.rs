//! macOS: a file's ACL copied to another through `copyfile(3)` with `COPYFILE_ACL`, for
//! [`super::keep`]. macOS ACLs are not extended attributes `listxattr` shows, and libSystem's
//! `copyfile` is the one call that copies them.
#![expect(unsafe_code, reason = "FFI to copyfile")]

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

/// Copies the ACL of `from` to `to`; with none on `from`, `to` keeps its own.
pub(super) fn copy_acl(from: &Path, to: &Path) -> io::Result<()> {
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both are NUL-terminated strings that outlive the call; a null state is allowed and
    // the call keeps no pointer.
    let done = unsafe {
        libc::copyfile(
            from.as_ptr(),
            to.as_ptr(),
            std::ptr::null_mut(),
            libc::COPYFILE_ACL,
        )
    };
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

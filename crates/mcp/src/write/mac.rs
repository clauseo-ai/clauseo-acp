//! macOS: extended attributes, for a download's `com.apple.quarantine` mark
//! (`browser_guard/downloads/marks.rs`); std has no call for them. The reads sit here too, so
//! one file holds the guard's attribute calls.
#![expect(
    unsafe_code,
    reason = "getxattr and setxattr, for a download's quarantine mark; libc binds them, std does not"
)]

use std::ffi::{CStr, CString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

fn c_path(file: &Path) -> io::Result<CString> {
    CString::new(file.as_os_str().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

/// `getxattr` into `buffer` (none: only the size), on the file itself, never a symlink's target.
fn get(file: &Path, name: &CStr, buffer: Option<&mut [u8]>) -> Option<usize> {
    let path = c_path(file).ok()?;
    let (pointer, len) = match buffer {
        Some(buffer) => (buffer.as_mut_ptr().cast(), buffer.len()),
        None => (std::ptr::null_mut(), 0),
    };
    // SAFETY: `path` and `name` are NUL-terminated C strings that outlive the call; `pointer`
    // is null with size 0 (only the value's size is asked for) or writable for `len` bytes.
    let size = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            pointer,
            len,
            0,
            libc::XATTR_NOFOLLOW,
        )
    };
    usize::try_from(size).ok()
}

/// Whether `file` has attribute `name` with a value.
pub(crate) fn has_xattr(file: &Path, name: &CStr) -> bool {
    get(file, name, None).is_some_and(|size| size > 0)
}

/// Attribute `name` of `file`, up to 1 KiB, for the tests.
#[cfg(test)]
pub(crate) fn read_xattr(file: &Path, name: &CStr) -> Option<Vec<u8>> {
    let mut buffer = vec![0_u8; 1024];
    let size = get(file, name, Some(&mut buffer))?;
    buffer.truncate(size);
    Some(buffer)
}

/// Sets attribute `name` of `file` (not of a symlink's target) to `value`.
pub(crate) fn set_xattr(file: &Path, name: &CStr, value: &[u8]) -> io::Result<()> {
    let path = c_path(file)?;
    // SAFETY: `path` and `name` are NUL-terminated C strings and `value` is readable for the
    // length passed; all three outlive the call, which keeps none of them.
    let status = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            libc::XATTR_NOFOLLOW,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

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

/// How many times [`read_xattr`] asks again when the value grew past the size it just read.
#[cfg(test)]
const READ_TRIES: usize = 4;

/// `getxattr` into `buffer` (none: only the size), on the file itself, never a symlink's target.
fn get(file: &Path, name: &CStr, buffer: Option<&mut [u8]>) -> io::Result<usize> {
    let path = c_path(file)?;
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
    // -1 on failure, with the reason in errno.
    usize::try_from(size).map_err(|_negative| io::Error::last_os_error())
}

/// Whether `file` has attribute `name` with a value.
pub(crate) fn has_xattr(file: &Path, name: &CStr) -> bool {
    get(file, name, None).is_ok_and(|size| size > 0)
}

/// Attribute `name` of `file`, for the tests: its size, then its value into a buffer of that
/// size. A value that grew in between fails the second call with `ERANGE`; the size is then read
/// again, [`READ_TRIES`] times at most, so a value rewritten without end reads as `None`.
#[cfg(test)]
pub(crate) fn read_xattr(file: &Path, name: &CStr) -> Option<Vec<u8>> {
    for _ in 0..READ_TRIES {
        let mut buffer = vec![0_u8; get(file, name, None).ok()?];
        match get(file, name, Some(&mut buffer)) {
            Ok(size) => {
                buffer.truncate(size);
                return Some(buffer);
            }
            Err(error) if error.raw_os_error() == Some(libc::ERANGE) => {}
            Err(_) => return None,
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::{read_xattr, set_xattr};

    /// A value of any size comes back whole, past the 1 KiB the first version read; a missing
    /// file reads as none.
    #[test]
    fn a_value_of_any_size_reads_back_whole() {
        let dir = std::env::temp_dir().join(format!("clauseo-mcp-xattr-{}", std::process::id()));
        crate::write::create_dir_all(&dir).unwrap();
        let file = dir.join("f");
        crate::write::write(&file, b"x").unwrap();
        let name = c"com.clauseo.test";
        for len in [0, 1, 1024, 1025, 40_000] {
            let value = vec![b'v'; len];
            set_xattr(&file, name, &value).unwrap();
            assert_eq!(read_xattr(&file, name), Some(value), "{len} bytes");
        }
        assert_eq!(read_xattr(&dir.join("missing"), name), None);
        crate::write::remove_dir_all(&dir).unwrap();
    }
}

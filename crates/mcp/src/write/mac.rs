//! macOS: extended attributes, for a download's `com.apple.quarantine` mark
//! (`browser_guard/downloads/marks.rs`), and `renameatx_np`, the copied download's rename that
//! never replaces a file (`write/unix.rs`); std has no call for them. The reads sit here too, so
//! one file holds the guard's attribute calls.
#![expect(
    unsafe_code,
    reason = "xattr calls and renameatx_np, for a download's quarantine mark and its no-replace \
              rename; libc binds them, std does not"
)]

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::AsRawFd;
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

/// `fgetxattr` of attribute `name` of the open `file`: its value, `None` when it has none or the
/// value changed size between the two reads.
pub(crate) fn fget_xattr(file: &std::fs::File, name: &CStr) -> Option<Vec<u8>> {
    let fd = file.as_raw_fd();
    // SAFETY: `fd` is open for the call (`file` is borrowed); `name` is a NUL-terminated C string;
    // a null buffer with size 0 asks only for the value's size.
    let size = unsafe { libc::fgetxattr(fd, name.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
    let size = usize::try_from(size).ok()?;
    let mut buffer = vec![0_u8; size];
    // SAFETY: as above, with `buffer` writable for `size` bytes.
    let read =
        unsafe { libc::fgetxattr(fd, name.as_ptr(), buffer.as_mut_ptr().cast(), size, 0, 0) };
    let read = usize::try_from(read).ok()?;
    buffer.truncate(read);
    Some(buffer)
}

/// `fsetxattr`: sets attribute `name` of the open `file` to `value`; the copy's mark is set on
/// the file this process made and holds open, so no path is resolved again.
pub(crate) fn fset_xattr(file: &std::fs::File, name: &CStr, value: &[u8]) -> io::Result<()> {
    // SAFETY: the fd is open for the call; `name` is NUL-terminated and `value` readable for the
    // length passed; the call keeps none of them.
    let status = unsafe {
        libc::fsetxattr(
            file.as_raw_fd(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// `renameatx_np(dir, from, dir, to, RENAME_EXCL)`: a rename within the folder `dir` holds that
/// fails with `EEXIST` rather than replace `to`.
pub(crate) fn rename_excl(
    dir: &std::os::fd::OwnedFd,
    from: &std::ffi::OsStr,
    to: &std::ffi::OsStr,
) -> io::Result<()> {
    let c_name = |name: &std::ffi::OsStr| {
        CString::new(name.as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    };
    let (from, to) = (c_name(from)?, c_name(to)?);
    let fd = dir.as_raw_fd();
    // SAFETY: `fd` is open for the call (`dir` is borrowed); both names are NUL-terminated C
    // strings that outlive it.
    let status =
        unsafe { libc::renameatx_np(fd, from.as_ptr(), fd, to.as_ptr(), libc::RENAME_EXCL) };
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

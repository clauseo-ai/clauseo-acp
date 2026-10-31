//! Windows: `SetFileAttributesW`, the one Win32 call that changes a file here, for
//! [`super::hide_dir`].
#![expect(unsafe_code, reason = "FFI to SetFileAttributesW")]

use std::io;
use std::path::Path;

use windows::Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, SetFileAttributesW};
use windows::core::HSTRING;

/// Sets the attributes of `path` to `attributes`, all of them: the caller passes the ones it
/// keeps. The path goes as it is, without a `\\?\` prefix, so one over 260 characters fails
/// (and [`super::hide_dir`] logs it, leaving the folder visible) unless the process is
/// long-path aware.
pub(super) fn set_attributes(path: &Path, attributes: u32) -> io::Result<()> {
    let wide = HSTRING::from(path);
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call, and the call
    // keeps no pointer to it.
    unsafe { SetFileAttributesW(&wide, FILE_FLAGS_AND_ATTRIBUTES(attributes)) }
        .map_err(io::Error::from)
}

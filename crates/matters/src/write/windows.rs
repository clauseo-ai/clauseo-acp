//! Windows: `SetFileAttributesW`, the one Win32 call that changes a file here, for
//! [`super::hide_dir`].
#![expect(unsafe_code, reason = "FFI to SetFileAttributesW")]

use std::io;
use std::path::Path;

use windows::Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, SetFileAttributesW};
use windows::core::HSTRING;

use crate::long_path::long_path;

/// Sets the attributes of `path` to `attributes`, all of them: the caller passes the ones it
/// keeps. The path goes through [`long_path`], since this call does not get std's own verbatim
/// prefix: a matter folder past 260 UTF-16 units is hidden too.
pub(super) fn set_attributes(path: &Path, attributes: u32) -> io::Result<()> {
    let wide = HSTRING::from(long_path(path).as_path());
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call, and the call
    // keeps no pointer to it.
    unsafe { SetFileAttributesW(&wide, FILE_FLAGS_AND_ATTRIBUTES(attributes)) }
        .map_err(win32_io_error)
}

/// `error` as an `io::Error` whose raw code is the Win32 code when the HRESULT wraps one
/// (`FACILITY_WIN32`, 0x8007xxxx), so `kind()` and `raw_os_error()` read as std's own do.
fn win32_io_error(error: windows::core::Error) -> io::Error {
    let hr = error.code().0.cast_unsigned();
    if hr & 0xFFFF_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((hr & 0xFFFF).cast_signed())
    } else {
        io::Error::from(error)
    }
}

#[cfg(test)]
mod tests {
    use windows::core::{Error, HRESULT};

    use super::win32_io_error;

    #[test]
    fn a_win32_error_keeps_its_win32_code_and_others_keep_their_hresult() {
        let denied = win32_io_error(Error::from_hresult(HRESULT::from_win32(5)));
        assert_eq!(denied.raw_os_error(), Some(5));
        assert_eq!(denied.kind(), std::io::ErrorKind::PermissionDenied);

        let missing = win32_io_error(Error::from_hresult(HRESULT::from_win32(2)));
        assert_eq!(missing.raw_os_error(), Some(2));
        assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);

        let e_fail = HRESULT(0x8000_4005_u32.cast_signed());
        let other = win32_io_error(Error::from_hresult(e_fail));
        assert_eq!(other.raw_os_error(), Some(e_fail.0));
    }
}

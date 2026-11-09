//! The io error for a Win32 error that arrives as a `windows` error. `io::Error::from` keeps the
//! HRESULT as the raw code, so the kind and the raw code would be wrong for a Win32 error.

use std::io;

/// `error` as an `io::Error` whose raw code is the Win32 code when the HRESULT wraps one
/// (`FACILITY_WIN32`, 0x8007xxxx), so `kind()` and `raw_os_error()` read as std's own do.
pub(super) fn win32_io_error(error: windows::core::Error) -> io::Error {
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

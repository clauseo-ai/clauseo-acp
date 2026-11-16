//! Windows: `SetFileAttributesW` for [`super::hide_dir`], `ReplaceFileW` for a rewrite of a
//! lawyer's file ([`super::Staged::rename`]), and `SetFileSecurityW` for a temp file whose ACL
//! refuses its removal ([`allow_delete`]): the Win32 calls that change a file here.
#![expect(
    unsafe_code,
    reason = "FFI to SetFileAttributesW, ReplaceFileW and SetFileSecurityW"
)]

use std::io;
use std::path::Path;

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SetFileSecurityW,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAGS_AND_ATTRIBUTES, REPLACEFILE_IGNORE_ACL_ERRORS, REPLACEFILE_IGNORE_MERGE_ERRORS,
    ReplaceFileW, SetFileAttributesW,
};
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

/// Puts the file `replacement` in the place of the file `replaced`, which keeps its attributes
/// (read-only, hidden), its ACL, its alternate data streams (`Zone.Identifier`, the mark of a
/// downloaded file), its creation time and its short name: what a rename over it drops. The old
/// file is moved to `backup` on the way, so a failure never leaves it nowhere
/// ([`super::replace_keeping`] reads what is on disk after one). An attribute or ACL that cannot
/// be merged does not fail it. All three paths go through [`long_path`], as in
/// [`set_attributes`].
pub(super) fn replace_file(replaced: &Path, replacement: &Path, backup: &Path) -> io::Result<()> {
    let replaced = HSTRING::from(long_path(replaced).as_path());
    let replacement = HSTRING::from(long_path(replacement).as_path());
    let backup = HSTRING::from(long_path(backup).as_path());
    let flags = REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS;
    // SAFETY: the three are NUL-terminated UTF-16 strings that outlive the call; the exclude and
    // reserved pointers are null, as the call requires; it keeps no pointer.
    unsafe { ReplaceFileW(&replaced, &replacement, &backup, flags, None, None) }
        .map_err(win32_io_error)
}

/// `FILE_ATTRIBUTE_NORMAL`: no attribute, read-only off.
const NORMAL: u32 = 0x80;

/// Lets the temp file `path` be removed: its ACL becomes one entry, its owner allowed
/// everything, not inherited (a deny-delete entry it inherited, or a folder that denies
/// deleting its files, refused it), and the read-only attribute goes.
pub(super) fn allow_delete(path: &Path) -> io::Result<()> {
    set_dacl(path, "D:P(A;;FA;;;OW)")?;
    set_attributes(path, NORMAL)
}

/// Sets the DACL of `path` to the one the SDDL string `sddl` gives, protected from inheritance.
pub(super) fn set_dacl(path: &Path, sddl: &str) -> io::Result<()> {
    let sddl = HSTRING::from(sddl);
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sddl` is a NUL-terminated UTF-16 string that outlives the call, which writes a
    // descriptor it allocated to `descriptor`; the size pointer may be absent.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &sddl,
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .map_err(win32_io_error)?;
    let wide = HSTRING::from(long_path(path).as_path());
    let what = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: `wide` is NUL-terminated and `descriptor` valid for the whole call, which keeps
    // no pointer to either.
    let set = unsafe { SetFileSecurityW(&wide, what, descriptor) }.ok();
    // SAFETY: `descriptor` was allocated by the conversion with `LocalAlloc` and is freed once.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    set.map_err(win32_io_error)
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

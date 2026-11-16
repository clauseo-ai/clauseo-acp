//! Windows: `SetFileAttributesW` for [`super::hide_dir`], `ReplaceFileW` for a rewrite of a
//! lawyer's file ([`super::Staged::rename`]), and, through a handle only, a temp file's identity
//! (`GetFileInformationByHandle`), and its DACL (`SetSecurityInfo`), attributes and deletion
//! (`SetFileInformationByHandle`) when its ACL refuses its removal ([`super::temps`]): the
//! Win32 calls that change a file here.
#![expect(
    unsafe_code,
    reason = "FFI to SetFileAttributesW, ReplaceFileW, and the handle calls for a temp file"
)]

use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;

use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetSecurityInfo,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_BASIC_INFO, FILE_DISPOSITION_INFO, FILE_FLAGS_AND_ATTRIBUTES, FileBasicInfo,
    FileDispositionInfo, GetFileInformationByHandle, REPLACEFILE_IGNORE_ACL_ERRORS,
    REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW, SetFileAttributesW, SetFileInformationByHandle,
};
use windows::core::{BOOL, HSTRING};

use super::temps::{FileId, Identity};
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

/// `READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES`: what
/// [`allow_delete`] needs of its handle.
pub(super) const UNLOCK_ACCESS: u32 = 0x0002_0000 | 0x0004_0000 | 0x80 | 0x100;
/// `DELETE | FILE_READ_ATTRIBUTES`: what [`delete`] needs of its handle.
pub(super) const DELETE_ACCESS: u32 = 0x0001_0000 | 0x80;
/// `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`.
const SHARE_ALL: u32 = 0x7;
/// `FILE_FLAG_OPEN_REPARSE_POINT`: a symlink or junction is opened itself, never followed.
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// Opens the existing `path` with `access`, sharing everything, without following a symlink or
/// junction at it; a folder (a junction included) fails to open, since the folder flag
/// (`FILE_FLAG_BACKUP_SEMANTICS`) is not given.
pub(super) fn open_no_follow(path: &Path, access: u32) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .access_mode(access)
        .share_mode(SHARE_ALL)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(path)
}

/// The identity of the open file `file`, from `GetFileInformationByHandle`: volume serial and
/// file index, links, and whether it is a regular file (no folder, no reparse point).
pub(super) fn identity(file: &File) -> io::Result<Identity> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open for the whole call, which writes `info` and keeps no pointer.
    unsafe { GetFileInformationByHandle(handle(file), &raw mut info) }.map_err(win32_io_error)?;
    let not_regular = FILE_ATTRIBUTE_DIRECTORY.0 | FILE_ATTRIBUTE_REPARSE_POINT.0;
    Ok(Identity {
        id: FileId {
            volume: info.dwVolumeSerialNumber.into(),
            index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        },
        links: info.nNumberOfLinks.into(),
        regular: info.dwFileAttributes & not_regular == 0,
    })
}

/// Lets the open temp file `file` (opened with [`UNLOCK_ACCESS`]) be removed, through its
/// handle: its DACL becomes one entry, its owner allowed everything, not inherited (a
/// deny-delete entry it inherited, or a folder that denies deleting its files, refused it), and
/// the read-only attribute goes. The caller has checked the handle is that temp file, with one
/// link ([`super::temps`]).
pub(super) fn allow_delete(file: &File) -> io::Result<()> {
    let descriptor = Descriptor::from_sddl("D:P(A;;FA;;;OW)")?;
    let mut present = BOOL::default();
    let mut defaulted = BOOL::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `descriptor` is valid for the whole call, which writes the three out-pointers;
    // `dacl` points into `descriptor`, which outlives its use below.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor.0,
            &raw mut present,
            &raw mut dacl,
            &raw mut defaulted,
        )
    }
    .map_err(win32_io_error)?;
    let what = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: the handle is open, and `dacl` valid (inside `descriptor`) for the whole call,
    // which keeps no pointer to it.
    let set = unsafe {
        SetSecurityInfo(
            handle(file),
            SE_FILE_OBJECT,
            what,
            None,
            None,
            Some(dacl),
            None,
        )
    };
    if set.0 != 0 {
        return Err(io::Error::from_raw_os_error(set.0.cast_signed()));
    }
    // Zero times leave them as they are.
    let basic = FILE_BASIC_INFO {
        FileAttributes: NORMAL,
        ..FILE_BASIC_INFO::default()
    };
    set_info(file, FileBasicInfo, &basic)
}

/// Deletes the open file `file` (opened with [`DELETE_ACCESS`]) through its handle: it goes
/// when the handle closes.
pub(super) fn delete(file: &File) -> io::Result<()> {
    set_info(
        file,
        FileDispositionInfo,
        &FILE_DISPOSITION_INFO { DeleteFile: true },
    )
}

/// `SetFileInformationByHandle` of `info`, of the class `class`, on `file`.
fn set_info<T>(
    file: &File,
    class: windows::Win32::Storage::FileSystem::FILE_INFO_BY_HANDLE_CLASS,
    info: &T,
) -> io::Result<()> {
    let size = u32::try_from(size_of::<T>()).unwrap_or(u32::MAX);
    // SAFETY: the handle is open, and `info` is a value of the struct `class` names, of
    // `size` bytes, for the whole call, which keeps no pointer to it.
    unsafe {
        SetFileInformationByHandle(handle(file), class, std::ptr::from_ref(info).cast(), size)
    }
    .map_err(win32_io_error)
}

/// The Win32 handle of `file`.
fn handle(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}

/// A security descriptor `ConvertStringSecurityDescriptorToSecurityDescriptorW` allocated,
/// freed when dropped.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// The descriptor the SDDL string `sddl` gives.
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let sddl = HSTRING::from(sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `sddl` is a NUL-terminated UTF-16 string that outlives the call, which writes
        // a descriptor it allocated to `descriptor`; the size pointer may be absent.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(win32_io_error)?;
        Ok(Self(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by the conversion with `LocalAlloc` and is freed
        // once.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// Tests: sets the DACL of `path` to the one the SDDL string `sddl` gives, protected from
/// inheritance.
#[cfg(test)]
pub(super) fn set_dacl(path: &Path, sddl: &str) -> io::Result<()> {
    use windows::Win32::Security::SetFileSecurityW;
    let descriptor = Descriptor::from_sddl(sddl)?;
    let wide = HSTRING::from(long_path(path).as_path());
    let what = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: `wide` is NUL-terminated and `descriptor` valid for the whole call, which keeps
    // no pointer to either.
    unsafe { SetFileSecurityW(&wide, what, descriptor.0) }
        .ok()
        .map_err(win32_io_error)
}

/// Tests: the DACL of `path` (not through a link; a folder too) as SDDL text.
#[cfg(test)]
pub(super) fn dacl_text(path: &Path) -> String {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo,
    };
    /// `READ_CONTROL`; `FILE_FLAG_BACKUP_SEMANTICS`, to open a folder.
    const READ_CONTROL: u32 = 0x0002_0000;
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let file = std::fs::OpenOptions::new()
        .access_mode(READ_CONTROL)
        .share_mode(SHARE_ALL)
        .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
        .open(path)
        .unwrap();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the handle is open for the whole call, which writes a descriptor it allocated.
    let got = unsafe {
        GetSecurityInfo(
            handle(&file),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            Some(&raw mut descriptor),
        )
    };
    assert_eq!(got.0, 0, "GetSecurityInfo");
    let descriptor = Descriptor(descriptor);
    let mut text = windows::core::PWSTR::null();
    // SAFETY: `descriptor` is valid; the call writes a string it allocated to `text`.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.0,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &raw mut text,
            None,
        )
    }
    .unwrap();
    // SAFETY: `text` is a NUL-terminated UTF-16 string, read before it is freed once.
    let owned = unsafe { text.to_string() }.unwrap();
    // SAFETY: `text` was allocated with `LocalAlloc` by the conversion.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    owned
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

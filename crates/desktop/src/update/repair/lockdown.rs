//! The attempt's folder once its installer is written and sealed, and whether the admin copy is
//! really missing, on Windows.
//!
//! - **The lock-down** ([`lock_down`]): the installer's elevated step re-runs `$EXEPATH` from
//!   this folder (`admin_copy.nsh:405`), so nothing may be added beside it to be loaded by name.
//!   The folder's DACL becomes, protected (nothing inherited from `updates/`): this user read and
//!   execute on the folder plus `FILE_DELETE_CHILD` (so the start's sweep can still delete the
//!   installer), read and execute inherited by the file; `OWNER RIGHTS` (S-1-3-4) read and execute,
//!   on both, so being the owner restores neither write nor `WRITE_DAC`; SYSTEM full control.
//!   `SetNamedSecurityInfoW` carries the inherited entries to the file. The held handle is
//!   untouched: share modes don't govern `WRITE_DAC`, and access is checked only at open.
//! - **Nothing but the installer** ([`entries`]): until the seal the folder was the user's, so a
//!   file dropped there during the download (a `version.dll`) would outlive the lock-down. After
//!   it, the folder is listed through a directory handle opened then (never `read_dir` on the
//!   path), and anything but the installer refuses the attempt (`plan::only_installer`).
//! - **The admin copy** ([`admin_copy_missing`]): the record names no admin copy (or can't be
//!   read, which the launch check treats as missing too), or no plain file is at
//!   `<Program Files>\Clauseo\Sandbox\clauseo-sandbox-setup.exe`.

#![expect(
    unsafe_code,
    reason = "FFI to SetNamedSecurityInfoW with a converted descriptor"
)]

use std::ffi::c_void;
use std::io;
use std::path::Path;

use clauseo_settings::{KnownFolder, PlatformDirs as _, SystemDirs};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetNamedSecurityInfoW,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};
use windows::core::HSTRING;

use super::plan;
use super::windows::own_sid;

/// The lock-down's DACL for `sid`: see the module's notes. `0x1200a9` is
/// `FILE_GENERIC_READ | FILE_GENERIC_EXECUTE`, `0x40` `FILE_DELETE_CHILD`.
pub(super) fn locked_sddl(sid: &str) -> String {
    format!(
        "D:P(A;;0x1200e9;;;{sid})(A;OIIO;0x1200a9;;;{sid})(A;OICI;0x1200a9;;;OW)(A;OICI;FA;;;SY)"
    )
}

/// Locks `folder` down (see the module's notes).
pub(super) fn lock_down(folder: &Path) -> io::Result<()> {
    let sddl = HSTRING::from(locked_sddl(&own_sid()?));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: a live string and out-pointer; the descriptor is freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &sddl,
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .map_err(io::Error::from)?;
    let set = set_dacl(folder, descriptor);
    // SAFETY: allocated by the conversion with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    set
}

/// Sets `folder`'s DACL to `descriptor`'s, protected.
fn set_dacl(folder: &Path, descriptor: PSECURITY_DESCRIPTOR) -> io::Result<()> {
    let (mut present, mut defaulted) = (windows::core::BOOL(0), windows::core::BOOL(0));
    let mut acl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `descriptor` is valid; the out-pointers are live.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &raw mut present,
            &raw mut acl,
            &raw mut defaulted,
        )
    }
    .map_err(io::Error::from)?;
    // SAFETY: a live path; `acl` points into `descriptor`, alive for the call.
    let set = unsafe {
        SetNamedSecurityInfoW(
            &HSTRING::from(folder.as_os_str()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(acl),
            None,
        )
    };
    if set.is_ok() {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(set.0.cast_signed()))
    }
}

/// The names in `folder`, `.` and `..` included, listed through a directory handle opened now
/// (`FILE_LIST_DIRECTORY`, sharing reads; `FILE_FLAG_BACKUP_SEMANTICS` opens a folder) with
/// `GetFileInformationByHandleEx`'s `FileIdBothDirectoryInfo`.
pub(super) fn entries(folder: &Path) -> io::Result<Vec<String>> {
    use std::os::windows::io::FromRawHandle as _;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_SHARE_READ,
        OPEN_EXISTING,
    };
    // SAFETY: a live HSTRING; no security attributes or template.
    let handle = unsafe {
        CreateFileW(
            &HSTRING::from(folder.as_os_str()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(io::Error::from)?;
    // SAFETY: a fresh handle nothing else owns; closed when `owned` drops.
    let owned = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle.0) };
    list(&owned)
}

/// Every name the directory `handle` lists, from the start.
fn list(handle: &std::os::windows::io::OwnedHandle) -> io::Result<Vec<String>> {
    use std::os::windows::io::AsRawHandle as _;
    use windows::Win32::Foundation::{ERROR_NO_MORE_FILES, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        FILE_ID_BOTH_DIR_INFO, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
        GetFileInformationByHandleEx,
    };
    let raw = HANDLE(handle.as_raw_handle());
    // u64s, so each record is aligned; 64 KiB per call.
    let mut buffer = vec![0u64; 8 * 1024];
    let size = u32::try_from(buffer.len() * size_of::<u64>()).unwrap_or(u32::MAX);
    let mut names = Vec::new();
    let mut class = FileIdBothDirectoryRestartInfo;
    loop {
        // SAFETY: `buffer` is live and writable for `size` bytes for the call.
        let read = unsafe {
            GetFileInformationByHandleEx(raw, class, buffer.as_mut_ptr().cast::<c_void>(), size)
        };
        match read {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => return Ok(names),
            Err(error) => return Err(io::Error::from(error)),
        }
        class = FileIdBothDirectoryInfo;
        let mut offset = 0usize;
        loop {
            // Records start 8-aligned (NextEntryOffset is a multiple of 8) and lie in the buffer.
            let whole = offset.is_multiple_of(size_of::<u64>())
                && offset + size_of::<FILE_ID_BOTH_DIR_INFO>() <= buffer.len() * size_of::<u64>();
            if !whole {
                return Err(io::Error::other("the folder's listing is malformed"));
            }
            // SAFETY: the call wrote a FILE_ID_BOTH_DIR_INFO at this 8-aligned offset, inside
            // `buffer` (checked above), which outlives `record`.
            let record = unsafe {
                &*buffer
                    .as_ptr()
                    .add(offset / size_of::<u64>())
                    .cast::<FILE_ID_BOTH_DIR_INFO>()
            };
            let units = usize::try_from(record.FileNameLength).unwrap_or(0) / 2;
            // SAFETY: the record's name is `FileNameLength` bytes from `FileName`, in the buffer.
            let name = unsafe { std::slice::from_raw_parts(record.FileName.as_ptr(), units) };
            names.push(String::from_utf16_lossy(name));
            if record.NextEntryOffset == 0 {
                break;
            }
            offset += usize::try_from(record.NextEntryOffset).unwrap_or(usize::MAX);
        }
    }
}

/// Whether the admin copy is missing on this machine (see the module's notes).
pub(super) fn admin_copy_missing() -> bool {
    use clauseo_sandbox::windows::admin_copy::admin_exe;
    use clauseo_sandbox::windows::refresh::recorded_admin_copy_sha256;
    let recorded = recorded_admin_copy_sha256()
        .map(|hash| hash.is_some())
        .map_err(drop);
    let present = SystemDirs
        .known_folder(KnownFolder::ProgramFiles)
        .and_then(|program_files| std::fs::symlink_metadata(admin_exe(&program_files)).ok())
        .is_some_and(|meta| super::sweep::plain_file(&meta));
    plan::copy_missing(recorded, present)
}

//! Windows: whether an agent sandbox that grants by ACL can prepare a folder. [`folder_support`]
//! asks the folder and its volume a few questions, in order, and stops at the first answer. It
//! writes nothing and takes no lock, and it answers for the calling process's own token, which
//! the agent inherits.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 file, volume and security calls"
)]

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, HANDLE,
};
use windows::Win32::Security::{
    AccessCheck, DACL_SECURITY_INFORMATION, DuplicateToken, GENERIC_MAPPING,
    GROUP_SECURITY_INFORMATION, GetFileSecurityW, LABEL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PRIVILEGE_SET, PSECURITY_DESCRIPTOR, SecurityImpersonation,
    TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_NAME_OPENED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GETFINALPATHNAMEBYHANDLE_FLAGS, GetDriveTypeW, GetFileInformationByHandle,
    GetFinalPathNameByHandleW, GetVolumeInformationByHandleW, OPEN_EXISTING, VOLUME_NAME_DOS,
    WRITE_DAC,
};
use windows::Win32::System::SystemServices::FILE_PERSISTENT_ACLS;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::WindowsProgramming::DRIVE_REMOTE;
use windows::core::{BOOL, PCWSTR};

/// Whether an agent sandbox that grants by ACL can prepare a folder on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderSupport {
    /// A local volume with permissions, and the caller can change the folder's permissions.
    Ok,
    /// The caller can't change the folder's permissions: the folder's DACL doesn't grant the
    /// caller `WRITE_DAC`, or the permissions can't be read.
    CantChangePermissions,
    /// The folder is on a network drive: a UNC path, a mapped drive, or a link that resolves to
    /// one.
    Network,
    /// The folder's volume keeps no permissions (FAT, exFAT).
    NoPermissions,
}

/// Asks whether the folder at `path` can take the agent's permission entries, in five steps (the
/// first that answers wins):
///
/// 1. `Network` when the path is a UNC path (`\\?\UNC\...`), or its drive letter is
///    `DRIVE_REMOTE`. Asked before the folder is opened, so an unreachable server can't fail it.
/// 2. Open the folder. A missing path is `NotFound`; a file is `InvalidInput`.
/// 3. `Network` when the folder's final path, from `FILE_NAME_OPENED | VOLUME_NAME_DOS`, is a UNC
///    path: a link to a share. If that call fails, this step is skipped, not an error.
/// 4. `NoPermissions` when the volume lacks `FILE_PERSISTENT_ACLS`.
/// 5. `CantChangePermissions` when the folder's DACL, checked with `AccessCheck` for this
///    process's token, doesn't grant `WRITE_DAC`, or its permissions can't be read. Otherwise
///    `Ok`.
///
/// Network folders go no further than their step: the server decides there. Every handle is
/// closed on every path. A path over `MAX_PATH` goes to the Win32 calls in the `\\?\` form; no
/// test makes one yet. The answer is for this process's token, which the agent inherits.
/// `AccessCheck` evaluates the DACL, the owner rights and the integrity label as
/// `SetNamedSecurityInfoW` will meet them. Backup and restore privileges don't affect it, so a
/// token that holds them gets the same answer as one that doesn't.
pub fn folder_support(path: &Path) -> io::Result<FolderSupport> {
    let wide = verbatim_wide(path)?;
    if is_share_path(&wide) || on_remote_drive(&wide) {
        return Ok(FolderSupport::Network);
    }
    let folder = open(&wide, FILE_READ_ATTRIBUTES.0).map_err(|error| os_error(&error))?;
    if !is_directory(&folder)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a folder", path.display()),
        ));
    }
    if final_path(&folder).is_some_and(|final_path| is_share_path(&final_path)) {
        return Ok(FolderSupport::Network);
    }
    if volume_flags(&folder)? & FILE_PERSISTENT_ACLS == 0 {
        return Ok(FolderSupport::NoPermissions);
    }
    let Some(mut descriptor) = security_descriptor(&wide)? else {
        return Ok(FolderSupport::CantChangePermissions);
    };
    let token = impersonation_token()?;
    if grants_write_dac(&mut descriptor, token.0)? {
        Ok(FolderSupport::Ok)
    } else {
        Ok(FolderSupport::CantChangePermissions)
    }
}

/// A file or folder handle, closed on drop.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful CreateFileW, OpenProcessToken or
        // DuplicateToken and is closed only here, once.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            tracing::debug!(code = error.code().0, "CloseHandle failed");
        }
    }
}

/// Opens `wide` (NUL-terminated, verbatim) for `access`, as a folder: `OPEN_EXISTING` with
/// `FILE_FLAG_BACKUP_SEMANTICS`, and all three share modes so the check never blocks a tool that
/// has the folder open.
fn open(wide: &[u16], access: u32) -> windows::core::Result<Handle> {
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    // SAFETY: `wide` is NUL-terminated UTF-16 that outlives the call; no security attributes or
    // template handle are passed, and the returned handle is owned by `Handle` from here.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            share,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }?;
    Ok(Handle(handle))
}

/// Whether the open handle is a folder.
fn is_directory(folder: &Handle) -> io::Result<bool> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open and `info` is a valid, writable struct for the call.
    unsafe { GetFileInformationByHandle(folder.0, &raw mut info) }
        .map_err(|error| os_error(&error))?;
    Ok(info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0)
}

/// The folder's final path in the `\\?\` form (`\\?\UNC\server\share\...` for a network path),
/// from `FILE_NAME_OPENED`: the name the handle was opened by, which needs none of the per-folder
/// lookups that a share can refuse. `None` when the call fails, which is not an answer.
fn final_path(folder: &Handle) -> Option<Vec<u16>> {
    let flags = GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_OPENED.0 | VOLUME_NAME_DOS.0);
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: the handle is open and `buffer` is a writable slice of the length given.
        let length = unsafe { GetFinalPathNameByHandleW(folder.0, &mut buffer, flags) };
        match usize::try_from(length).ok()? {
            0 => return None,
            // A length within the buffer is the path without its NUL; a longer one is the size
            // the path needs, NUL included, so the next call fits.
            n if n < buffer.len() => {
                buffer.truncate(n);
                return Some(buffer);
            }
            n => buffer.resize(n + 1, 0),
        }
    }
}

/// Whether `wide` (a verbatim path) is a UNC path, which names a network folder.
fn is_share_path(wide: &[u16]) -> bool {
    wide.starts_with(&wide_text(r"\\?\UNC\"))
}

/// Whether the drive letter of `wide` (a verbatim path, so the letter is at index 4) is a
/// `DRIVE_REMOTE` drive. The drive is asked by its root, `X:\`, so no folder is opened. A path
/// without a drive letter is not answered here.
fn on_remote_drive(wide: &[u16]) -> bool {
    let Some(&[letter, colon]) = wide.get(4..6) else {
        return false;
    };
    let is_letter = u8::try_from(letter).is_ok_and(|byte| byte.is_ascii_alphabetic());
    if !is_letter || colon != u16::from(b':') {
        return false;
    }
    let root = [letter, colon, u16::from(b'\\'), 0];
    // SAFETY: `root` is NUL-terminated UTF-16 that outlives the call.
    unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) == DRIVE_REMOTE }
}

/// The volume's file-system flags.
fn volume_flags(folder: &Handle) -> io::Result<u32> {
    let mut flags = 0_u32;
    // SAFETY: the handle is open; `flags` is a valid, writable u32, and the buffers are left out.
    unsafe {
        GetVolumeInformationByHandleW(folder.0, None, None, None, Some(&raw mut flags), None)
    }
    .map_err(|error| os_error(&error))?;
    Ok(flags)
}

/// The folder's owner, group, DACL and integrity label, as a self-relative security descriptor in
/// u64 words, so its structures are aligned. `None` when the caller can't read them
/// (`ERROR_ACCESS_DENIED`).
fn security_descriptor(wide: &[u16]) -> io::Result<Option<Vec<u64>>> {
    // The label too, so AccessCheck applies the folder's integrity label (no write-up) as the
    // real grant meets it; reading it needs only READ_CONTROL, as the rest does.
    const INFORMATION: u32 = OWNER_SECURITY_INFORMATION.0
        | GROUP_SECURITY_INFORMATION.0
        | DACL_SECURITY_INFORMATION.0
        | LABEL_SECURITY_INFORMATION.0;
    let mut needed = 0_u32;
    // SAFETY: `wide` is NUL-terminated and outlives the call; with no buffer the call only
    // reports the size, into `needed`, a live out-pointer.
    let sized =
        unsafe { GetFileSecurityW(PCWSTR(wide.as_ptr()), INFORMATION, None, 0, &raw mut needed) };
    if sized.as_bool() {
        return Err(io::Error::other(
            "GetFileSecurityW succeeded with no buffer",
        ));
    }
    // The size call fails with ERROR_INSUFFICIENT_BUFFER; a denial here is the answer.
    let error = windows::core::Error::from_thread();
    if error.code() == ERROR_ACCESS_DENIED.to_hresult() {
        return Ok(None);
    }
    if error.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult() {
        return Err(os_error(&error));
    }
    let mut descriptor = words_for(needed)?;
    let mut size = needed;
    for _ in 0..DESCRIPTOR_READS {
        let mut returned = 0_u32;
        // SAFETY: `wide` is NUL-terminated and outlives the call; `descriptor` is a writable buffer
        // of `size` bytes or more, and `returned` a live out-pointer.
        let read = unsafe {
            GetFileSecurityW(
                PCWSTR(wide.as_ptr()),
                INFORMATION,
                Some(PSECURITY_DESCRIPTOR(descriptor.as_mut_ptr().cast())),
                size,
                &raw mut returned,
            )
        };
        if read.as_bool() {
            return Ok(Some(descriptor));
        }
        let error = windows::core::Error::from_thread();
        if error.code() == ERROR_ACCESS_DENIED.to_hresult() {
            return Ok(None);
        }
        if error.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult() {
            return Err(os_error(&error));
        }
        // An entry was added since the size call: read again at the length this call reported.
        size = returned;
        descriptor = words_for(size)?;
    }
    Err(io::Error::other(
        "the folder's permissions kept growing while they were read",
    ))
}

/// A duplicate of this process's token as an impersonation token, which `AccessCheck` takes. The
/// process token and the duplicate are both closed on drop.
fn impersonation_token() -> io::Result<Handle> {
    let mut process = HANDLE::default();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `process` is a live
    // out-pointer, and the token it receives is closed by `Handle` below.
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_DUPLICATE | TOKEN_QUERY,
            &raw mut process,
        )
    }
    .map_err(|error| os_error(&error))?;
    let process = Handle(process);
    let mut duplicate = HANDLE::default();
    // SAFETY: `process.0` is the open token, which lives until `process` drops at the end of
    // this function; `duplicate` is a live out-pointer.
    unsafe { DuplicateToken(process.0, SecurityImpersonation, &raw mut duplicate) }
        .map_err(|error| os_error(&error))?;
    Ok(Handle(duplicate))
}

/// Whether `token` is granted `WRITE_DAC` by the security descriptor `descriptor`, from
/// `AccessCheck`. The privilege-set buffer starts at 256 bytes; if `AccessCheck` reports it too
/// small, it is sized to the length it reports and the check runs once more.
fn grants_write_dac(descriptor: &mut [u64], token: HANDLE) -> io::Result<bool> {
    let mapping = GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ.0,
        GenericWrite: FILE_GENERIC_WRITE.0,
        GenericExecute: FILE_GENERIC_EXECUTE.0,
        GenericAll: FILE_ALL_ACCESS.0,
    };
    let mut privileges = words_for(PRIVILEGE_BYTES)?;
    let mut length = PRIVILEGE_BYTES;
    let mut granted = 0_u32;
    let mut status = BOOL(0);
    for attempt in 0..2 {
        // SAFETY: `descriptor` and `privileges` are writable buffers that live for the call, and
        // `length` is the size of the privilege set buffer; `mapping`, `granted` and `status`
        // are live for the call too.
        let call = unsafe {
            AccessCheck(
                PSECURITY_DESCRIPTOR(descriptor.as_mut_ptr().cast()),
                token,
                WRITE_DAC.0,
                &raw const mapping,
                Some(privileges.as_mut_ptr().cast::<PRIVILEGE_SET>()),
                &raw mut length,
                &raw mut granted,
                &raw mut status,
            )
        };
        match call {
            Ok(()) => return Ok(status.as_bool()),
            Err(error)
                if attempt == 0 && error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() =>
            {
                privileges = words_for(length)?;
            }
            Err(error) => return Err(os_error(&error)),
        }
    }
    Err(io::Error::other(
        "AccessCheck asked for a larger privilege set twice",
    ))
}

/// A zeroed buffer of u64 words, enough for `bytes` bytes.
fn words_for(bytes: u32) -> io::Result<Vec<u64>> {
    let bytes = usize::try_from(bytes).map_err(io::Error::other)?;
    Ok(vec![0_u64; bytes.div_ceil(size_of::<u64>())])
}

/// `path` as an absolute, NUL-terminated UTF-16 string in the `\\?\` form, which lifts the
/// `MAX_PATH` limit. A UNC path becomes `\\?\UNC\...`.
fn verbatim_wide(path: &Path) -> io::Result<Vec<u16>> {
    let absolute = std::path::absolute(path)?;
    let text: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let verbatim = wide_text(r"\\?\");
    let mut wide = if text.starts_with(&verbatim) {
        text
    } else if let Some(rest) = text.strip_prefix(wide_text(r"\\.\").as_slice()) {
        // A device path (`\\.\C:\x`, `\\.\UNC\srv\share`) keeps its rest, under `\\?\`.
        let mut wide = verbatim;
        wide.extend_from_slice(rest);
        wide
    } else if let Some(rest) = text.strip_prefix(wide_text(r"\\").as_slice()) {
        let mut wide = wide_text(r"\\?\UNC\");
        wide.extend_from_slice(rest);
        wide
    } else {
        let mut wide = verbatim;
        wide.extend_from_slice(&text);
        wide
    };
    wide.push(0);
    Ok(wide)
}

/// `text` as UTF-16, without a NUL.
fn wide_text(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// The privilege-set buffer's first size, in bytes: enough for a token with no privileges to
/// check, and `AccessCheck` names the size it needs when this is too small.
const PRIVILEGE_BYTES: u32 = 256;

/// How many times the descriptor is read when its size grows between the two `GetFileSecurityW`
/// calls.
const DESCRIPTOR_READS: usize = 4;

/// The Win32 error that `error` carries, as an `io::Error`. The calls here report
/// `HRESULT_FROM_WIN32(code)`, which keeps the code in its low 16 bits.
fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xFFFF)
}

#[cfg(test)]
mod tests;

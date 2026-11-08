//! The admin-only log on Windows, for [`Sweeper::admin_log`] (`log_path.rs`): the paths of the
//! tasks that keep the sweep at 56, which the Application log (every local user can read it)
//! no longer names. The file is `%ProgramData%\Clauseo\Sandbox\sweep-admin.log`, `%ProgramData%`
//! as Windows knows it (`FOLDERID_ProgramData`), never from the environment.
//!
//! `Clauseo` is made, when missing, as the installer makes it ([`CLAUSEO_FOLDER_SDDL`], Users
//! may read it) and `Sandbox` with [`ADMIN_FOLDER_SDDL`]; each is then
//! opened without following a reparse point (one, or a file, is refused) and its owner and DACL
//! read through that handle: they must let only SYSTEM, Administrators or `TrustedInstaller`
//! change it (`admin_copy::level_safe`). A standard user may make folders in `%ProgramData%`,
//! so one who made `Clauseo` first keeps the log from being written, never redirects it. The
//! file is opened the same way, made with [`ADMIN_LOG_SDDL`] when missing, and must be a plain
//! file with one link whose owner and DACL keep it to SYSTEM and Administrators
//! ([`admin_only`]). A line that would take it past [`ADMIN_LOG_CAP`] first moves it to
//! `sweep-admin.log.1`, replacing the one there. Each line starts with the UTC time.
//!
//! [`Sweeper::admin_log`]: super::super::Sweeper::admin_log
//! [`CLAUSEO_FOLDER_SDDL`]: super::super::log_path::CLAUSEO_FOLDER_SDDL
//! [`ADMIN_FOLDER_SDDL`]: super::super::log_path::ADMIN_FOLDER_SDDL
#![expect(
    unsafe_code,
    reason = "the file, folder, security and clock calls that write the admin-only log"
)]

use std::fs::File;
use std::io::Write as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
use std::path::Path;

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, HANDLE};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetKernelObjectSecurity, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, CreateFileW, FILE_APPEND_DATA,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_CREATION_DISPOSITION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, GetFileInformationByHandle,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, OPEN_ALWAYS, OPEN_EXISTING,
    READ_CONTROL, SYNCHRONIZE,
};
use windows::Win32::System::SystemInformation::GetSystemTime;
use windows::core::HSTRING;

use super::super::Failed;
use super::super::log_path::{
    ADMIN_LOG_CAP, ADMIN_LOG_FOLDERS, ADMIN_LOG_NAME, ADMIN_LOG_SDDL, admin_only, rotates,
};
use crate::windows::admin_copy::level_safe;
use crate::windows::setup::ffi::program_data;
use crate::windows::setup::lock::Descriptor;

/// Appends `line` to `%ProgramData%\Clauseo\Sandbox\sweep-admin.log`, as the module says.
pub(crate) fn append(line: &str) -> Result<(), Failed> {
    let base = program_data().map_err(|error| format!("the ProgramData folder: {error}"))?;
    append_in(&base, line)
}

/// [`append`] below `base` instead of `%ProgramData%`: the elevated test's own folder.
pub(crate) fn append_in(base: &Path, line: &str) -> Result<(), Failed> {
    let mut folder = base.to_path_buf();
    for (part, sddl) in ADMIN_LOG_FOLDERS {
        folder.push(part);
        admin_folder(&folder, sddl)?;
    }
    let log = folder.join(ADMIN_LOG_NAME);
    let text = format!("{} {line}\r\n", stamp());
    let add = u64::try_from(text.len()).unwrap_or(ADMIN_LOG_CAP);
    let (mut file, size) = open_log(&log)?;
    if rotates(size, add) {
        drop(file);
        rotate(&log, &folder.join(format!("{ADMIN_LOG_NAME}.1")))?;
        (file, _) = open_log(&log)?;
    }
    file.write_all(text.as_bytes())
        .map_err(|error| format!("the admin-only log couldn't be written ({error})"))
}

/// Makes the folder `path` with `sddl` when missing, then refuses it unless it
/// is a folder, not a reparse point, that only SYSTEM, Administrators or `TrustedInstaller` may
/// change.
fn admin_folder(path: &Path, sddl: &str) -> Result<(), Failed> {
    let descriptor = Descriptor::from_sddl(sddl).map_err(|error| error.to_string())?;
    let attributes = attributes(&descriptor);
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: `name` is NUL-terminated and `attributes` points at a live descriptor; both
    // outlive the call.
    let made = unsafe { CreateDirectoryW(&name, Some(&raw const attributes)) };
    if let Err(error) = made
        && code(&error) != ERROR_ALREADY_EXISTS.0
    {
        return Err(failed("CreateDirectoryW", &error));
    }
    let flags = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
    let folder = open(
        path,
        READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0,
        None,
        OPEN_EXISTING,
        flags,
    )?;
    let info = info(&folder)?;
    let attributes = info.dwFileAttributes;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || attributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
    {
        return Err("a folder of the admin-only log is a reparse point or a file".to_owned());
    }
    if !level_safe(&security_of(&folder)?) {
        return Err(
            "a folder of the admin-only log lets someone other than SYSTEM or Administrators \
             change it"
                .to_owned(),
        );
    }
    Ok(())
}

/// The log `path`, opened for appending (made with [`ADMIN_LOG_SDDL`] when missing), and its
/// size; refused unless a plain file with one link that [`admin_only`] keeps.
fn open_log(path: &Path) -> Result<(File, u64), Failed> {
    let descriptor = Descriptor::from_sddl(ADMIN_LOG_SDDL).map_err(|error| error.to_string())?;
    let attributes = attributes(&descriptor);
    let access = FILE_APPEND_DATA.0 | READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;
    let flags = FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT;
    let file = open(path, access, Some(&attributes), OPEN_ALWAYS, flags)?;
    let info = info(&file)?;
    let kind = FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0;
    if info.dwFileAttributes & kind != 0 || info.nNumberOfLinks != 1 {
        return Err("the admin-only log isn't a plain file with one link".to_owned());
    }
    if !admin_only(&security_of(&file)?) {
        return Err("the admin-only log isn't kept to SYSTEM and Administrators".to_owned());
    }
    let size = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
    Ok((file, size))
}

/// Moves the log `from` to `to`, replacing what is there.
fn rotate(from: &Path, to: &Path) -> Result<(), Failed> {
    let (from, to) = (
        HSTRING::from(from.as_os_str()),
        HSTRING::from(to.as_os_str()),
    );
    // SAFETY: both names are NUL-terminated and outlive the call.
    unsafe {
        MoveFileExW(
            &from,
            &to,
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| failed("MoveFileExW", &error))
}

/// `SECURITY_ATTRIBUTES` that give a new file or folder `descriptor`, which must outlive them.
fn attributes(descriptor: &Descriptor) -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    }
}

/// `path` itself opened with `access`, sharing read, made with `attributes` when `how` makes
/// it; owned by the [`File`] returned, closed on drop.
fn open(
    path: &Path,
    access: u32,
    attributes: Option<&SECURITY_ATTRIBUTES>,
    how: FILE_CREATION_DISPOSITION,
    flags: FILE_FLAGS_AND_ATTRIBUTES,
) -> Result<File, Failed> {
    let name = HSTRING::from(path.as_os_str());
    let attributes = attributes.map(std::ptr::from_ref);
    // SAFETY: `name` is NUL-terminated and any `attributes` point at a live descriptor; both
    // outlive the call. The handle is owned by the `File` at once.
    let handle =
        unsafe { CreateFileW(&name, access, FILE_SHARE_READ, attributes, how, flags, None) }
            .map_err(|error| failed("CreateFileW", &error))?;
    // SAFETY: a handle CreateFileW just opened, owned by nothing else.
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

/// What `file` is: its attributes, link count and size.
fn info(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION, Failed> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open; `info` is a live out-pointer.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &raw mut info) }
        .map_err(|error| failed("GetFileInformationByHandle", &error))?;
    Ok(info)
}

/// The owner and DACL of what `file` is open on (with `READ_CONTROL`), self-relative.
fn security_of(file: &File) -> Result<Vec<u8>, Failed> {
    let info = (OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION).0;
    let mut size = 256_u32;
    for _ in 0..4 {
        let mut buffer = vec![0_u64; usize::try_from(size).unwrap_or(0).div_ceil(8)];
        let mut needed = 0_u32;
        // SAFETY: the handle is open; `buffer` is writable for `size` bytes or more, and
        // `needed` is a live out-pointer.
        let read = unsafe {
            GetKernelObjectSecurity(
                HANDLE(file.as_raw_handle()),
                info,
                Some(PSECURITY_DESCRIPTOR(buffer.as_mut_ptr().cast())),
                size,
                &raw mut needed,
            )
        };
        match read {
            Ok(()) => {
                let mut bytes: Vec<u8> = buffer.iter().flat_map(|w| w.to_ne_bytes()).collect();
                bytes.truncate(usize::try_from(size).unwrap_or(0));
                return Ok(bytes);
            }
            Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => size = needed,
            Err(error) => return Err(failed("GetKernelObjectSecurity", &error)),
        }
    }
    Err("a security descriptor kept growing while it was read".to_owned())
}

/// The UTC time now, as `YYYY-MM-DDTHH:MM:SSZ`.
fn stamp() -> String {
    // SAFETY: no arguments; it returns the time by value.
    let now = unsafe { GetSystemTime() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond
    )
}

/// The Win32 code of `error` (the HRESULT's low word).
fn code(error: &windows::core::Error) -> u32 {
    error.code().0.cast_unsigned() & 0xFFFF
}

/// `call` failed with `error`, as a [`Failed`] naming no path.
fn failed(call: &str, error: &windows::core::Error) -> Failed {
    format!("{call} failed ({})", code(error))
}

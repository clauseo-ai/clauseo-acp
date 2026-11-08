//! `--wsl-deny`'s run lock and "run again" flag on Windows (`run_lock.rs`).
//!
//! The lock is `%ProgramFiles%\Clauseo\Sandbox\locks\wsl-deny.lock`. The admin folder must pass
//! the admin copy's check first (`admin_copy::check`, `%ProgramFiles%` from
//! `FOLDERID_ProgramFiles`, never the environment). `locks\` is made with [`LOCKS_SDDL`] at once
//! when missing (`CreateDirectoryW`), then opened without following a reparse point (one, or a
//! file, is refused) and its owner and DACL read through that handle (`admin_copy::level_safe`);
//! the file is made with [`LOCK_SDDL`] at once, opened the same way, then refused unless a plain
//! file with one link that [`system_only`] keeps. The handle code is a port of #750's
//! `windows_sweeper/admin_log.rs`, which isn't in this branch's base.
//!
//! The handle asks for `FILE_READ_DATA`: sharing is only checked between opens that ask for
//! data access, so a second run's open, asking the same, fails with a sharing violation
//! ([`Locked::Busy`]). A pool user can't open the file at all (its DACL), so can't hold it.
//!
//! The flag is the `REG_DWORD` [`AGAIN_VALUE`] in the record key (`HKLM`, the 64-bit view),
//! which only SYSTEM and Administrators may write. Clearing it is one delete, so a set that
//! comes as it is read is never lost.
#![expect(
    unsafe_code,
    reason = "the file, folder, security and registry calls of the run lock and its flag"
)]

use std::fs::File;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
use std::path::Path;
use std::ptr::null_mut;

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_SHARING_VIOLATION,
    ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetKernelObjectSecurity, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_CREATION_DISPOSITION,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES,
    FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_MODE, FILE_SHARE_READ,
    GetFileInformationByHandle, OPEN_ALWAYS, OPEN_EXISTING, READ_CONTROL,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_DWORD,
    REG_SAM_FLAGS, RRF_RT_REG_DWORD, RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegDeleteValueW,
    RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

use super::super::Failed;
use super::run_lock::{AGAIN_VALUE, LOCK_NAME, LOCK_SDDL, LOCKS_SDDL, Locked, system_only};
use crate::windows::admin_copy::os::WindowsAdminCopy;
use crate::windows::admin_copy::{self, LOCKS, level_safe};

/// Takes the run lock: the open file when held, as the module says.
pub(in crate::windows::refresh) fn take() -> (Locked, Option<File>) {
    match take_in() {
        Ok(Some(file)) => (Locked::Held, Some(file)),
        Ok(None) => (Locked::Busy, None),
        Err(why) => (Locked::Refused(why), None),
    }
}

/// [`take`]: `None` when another run holds it.
fn take_in() -> Result<Option<File>, Failed> {
    let exe = admin_copy::check(&WindowsAdminCopy)
        .map_err(|blocked| format!("the admin copy failed its check ({blocked})"))?;
    let folder = exe
        .parent()
        .ok_or("the admin copy has no folder")?
        .join(LOCKS);
    lock_folder(&folder, LOCKS_SDDL)?;
    let descriptor = Descriptor::from_sddl(LOCK_SDDL)?;
    let attributes = attributes(&descriptor);
    let access = FILE_READ_DATA.0 | READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0;
    let flags = FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT;
    let path = folder.join(LOCK_NAME);
    let opened = open(
        &path,
        access,
        FILE_SHARE_MODE(0),
        Some(&attributes),
        OPEN_ALWAYS,
        flags,
    );
    let file = match opened {
        Ok(file) => file,
        Err(error) if code(&error) == ERROR_SHARING_VIOLATION.0 => return Ok(None),
        Err(error) => return Err(failed("CreateFileW", &error)),
    };
    let info = info(&file)?;
    let kind = FILE_ATTRIBUTE_REPARSE_POINT.0 | FILE_ATTRIBUTE_DIRECTORY.0;
    if info.dwFileAttributes & kind != 0 || info.nNumberOfLinks != 1 {
        return Err("the run lock isn't a plain file with one link".to_owned());
    }
    if !system_only(&security_of(&file)?) {
        return Err("the run lock isn't kept to SYSTEM".to_owned());
    }
    Ok(Some(file))
}

/// Makes the folder `path` with `sddl` when missing, then refuses it unless it is a folder, not
/// a reparse point, that only SYSTEM, Administrators or `TrustedInstaller` may change.
fn lock_folder(path: &Path, sddl: &str) -> Result<(), Failed> {
    let descriptor = Descriptor::from_sddl(sddl)?;
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
    let access = READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0;
    let flags = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
    let folder = open(path, access, FILE_SHARE_READ, None, OPEN_EXISTING, flags)
        .map_err(|error| failed("CreateFileW", &error))?;
    let attributes = info(&folder)?.dwFileAttributes;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || attributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
    {
        return Err("a folder of the run lock is a reparse point or a file".to_owned());
    }
    if !level_safe(&security_of(&folder)?) {
        return Err(
            "a folder of the run lock lets someone other than SYSTEM or Administrators change it"
                .to_owned(),
        );
    }
    Ok(())
}

/// A security descriptor from `ConvertStringSecurityDescriptorToSecurityDescriptorW`, freed on
/// drop.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// The self-relative descriptor `sddl` describes.
    fn from_sddl(sddl: &str) -> Result<Descriptor, Failed> {
        let text = HSTRING::from(sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
        // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` is a live
        // out-pointer, and the descriptor it receives is freed on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(|error| {
            failed(
                "ConvertStringSecurityDescriptorToSecurityDescriptorW",
                &error,
            )
        })?;
        Ok(Descriptor(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: the descriptor came from the conversion, which allocates it with
            // LocalAlloc, and is freed once, here.
            let _freed = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }
}

/// `SECURITY_ATTRIBUTES` that give a new file or folder `descriptor`, which must outlive them.
fn attributes(descriptor: &Descriptor) -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    }
}

/// `path` itself opened with `access` and `share`, made with `attributes` when `how` makes it;
/// owned by the [`File`] returned, closed on drop.
fn open(
    path: &Path,
    access: u32,
    share: FILE_SHARE_MODE,
    attributes: Option<&SECURITY_ATTRIBUTES>,
    how: FILE_CREATION_DISPOSITION,
    flags: FILE_FLAGS_AND_ATTRIBUTES,
) -> Result<File, windows::core::Error> {
    let name = HSTRING::from(path.as_os_str());
    let attributes = attributes.map(std::ptr::from_ref);
    // SAFETY: `name` is NUL-terminated and any `attributes` point at a live descriptor; both
    // outlive the call. The handle is owned by the `File` at once.
    let handle = unsafe { CreateFileW(&name, access, share, attributes, how, flags, None) }?;
    // SAFETY: a handle CreateFileW just opened, owned by nothing else.
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

/// What `file` is: its attributes and link count.
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

/// Sets the flag in `HKLM\<record>`, which must exist.
pub(in crate::windows::refresh) fn ask_again(record: &str) -> Result<(), Failed> {
    let key = Key::open(record, KEY_SET_VALUE)?;
    let name = HSTRING::from(AGAIN_VALUE);
    let data = 1_u32.to_le_bytes();
    // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated and `data` a live
    // slice, a DWORD.
    let set = unsafe { RegSetValueExW(key.0, &name, None, REG_DWORD, Some(&data)) };
    win32("RegSetValueExW", set)
}

/// Clears the flag in `HKLM\<record>`; whether it was there.
pub(in crate::windows::refresh) fn take_again(record: &str) -> Result<bool, Failed> {
    let key = Key::open(record, KEY_SET_VALUE)?;
    let name = HSTRING::from(AGAIN_VALUE);
    // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated.
    let deleted = unsafe { RegDeleteValueW(key.0, &name) };
    if deleted == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    win32("RegDeleteValueW", deleted).map(|()| true)
}

/// Whether the flag is in `HKLM\<record>`, leaving it.
pub(in crate::windows::refresh) fn asked_again(record: &str) -> Result<bool, Failed> {
    let (subkey, name) = (HSTRING::from(record), HSTRING::from(AGAIN_VALUE));
    let mut data = 0_u32;
    let mut size = u32::try_from(size_of::<u32>()).unwrap_or(4);
    // SAFETY: both names are NUL-terminated; `data` is writable for `size` bytes, and both
    // out-pointers are live for the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &name,
            RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY,
            None,
            Some((&raw mut data).cast()),
            Some(&raw mut size),
        )
    };
    if read == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    win32("RegGetValueW", read).map(|()| true)
}

/// A key under `HKLM` (the 64-bit view), closed on drop.
struct Key(HKEY);

impl Key {
    /// `HKLM\<subkey>` opened for `access`.
    fn open(subkey: &str, access: REG_SAM_FLAGS) -> Result<Key, Failed> {
        let subkey = HSTRING::from(subkey);
        let mut key = HKEY::default();
        // SAFETY: the subkey is NUL-terminated and outlives the call; `key` is a live
        // out-pointer, owned by the `Key` on success.
        let opened = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                &subkey,
                None,
                access | KEY_QUERY_VALUE | KEY_WOW64_64KEY,
                &raw mut key,
            )
        };
        win32("RegOpenKeyExW", opened)?;
        Ok(Key(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key came from RegOpenKeyExW and is closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// `call`'s result `code`, as a [`Failed`] naming no path when it failed.
fn win32(call: &str, code: WIN32_ERROR) -> Result<(), Failed> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{call} failed ({})", code.0))
    }
}

/// The Win32 code of `error` (the HRESULT's low word).
fn code(error: &windows::core::Error) -> u32 {
    error.code().0.cast_unsigned() & 0xFFFF
}

/// `call` failed with `error`, as a [`Failed`] naming no path.
fn failed(call: &str, error: &windows::core::Error) -> Failed {
    format!("{call} failed ({})", code(error))
}

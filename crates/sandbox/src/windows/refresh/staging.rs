//! The refresh's file effects under Program Files (steps 6 and 9): the staging folder, made or
//! re-secured with a protected SYSTEM-only DACL and read back; the staged copy, written there
//! and read back; the admin copy's DACL given to it; and the renames, all write-through.
#![expect(
    unsafe_code,
    reason = "FFI to the file, security descriptor and rename calls"
)]

use std::path::Path;
use std::ptr::null_mut;

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, GENERIC_WRITE,
    HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_FILE_OBJECT, SetSecurityInfo,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES,
};
use windows::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, DeleteFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_WRITE_THROUGH,
    FILE_SHARE_MODE, FileAttributeTagInfo, FlushFileBuffers, GetFileInformationByHandleEx,
    MOVEFILE_DELAY_UNTIL_REBOOT, MOVEFILE_WRITE_THROUGH, MoveFileExW, OPEN_EXISTING,
    RemoveDirectoryW, WriteFile,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::Failed;
use super::source::{Handle, code_of};

/// `READ_CONTROL`, `WRITE_DAC` and `WRITE_OWNER`: what reading and setting a descriptor need.
const SECURITY_ACCESS: u32 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000;

/// A security descriptor from SDDL, allocated with `LocalAlloc`, freed on drop.
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

    /// Its owner and DACL, pointers into the descriptor.
    fn parts(&self) -> Result<(PSID, *const ACL), Failed> {
        let mut owner = PSID::default();
        let mut defaulted = windows::core::BOOL::default();
        // SAFETY: the descriptor is live; the out-pointers are live locals.
        unsafe { GetSecurityDescriptorOwner(self.0, &raw mut owner, &raw mut defaulted) }
            .map_err(|error| failed("GetSecurityDescriptorOwner", &error))?;
        let mut present = windows::core::BOOL::default();
        let mut dacl: *mut ACL = null_mut();
        // SAFETY: as above.
        unsafe {
            GetSecurityDescriptorDacl(self.0, &raw mut present, &raw mut dacl, &raw mut defaulted)
        }
        .map_err(|error| failed("GetSecurityDescriptorDacl", &error))?;
        if owner.0.is_null() || !present.as_bool() || dacl.is_null() {
            return Err("the descriptor has no owner or no DACL".to_owned());
        }
        Ok((owner, dacl.cast_const()))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: allocated with LocalAlloc by the conversion, or by GetSecurityInfo, and
            // freed here, once.
            unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }
}

/// `call` failed with `error`, as a log line.
fn failed(call: &str, error: &windows::core::Error) -> Failed {
    format!("{call} failed ({})", code_of(error))
}

/// Makes the folder `path` with the owner and protected DACL `sddl` describes, or, when it is
/// there, opens it without following a reparse point (refusing one, or a file) and sets them
/// again; then reads them back and refuses a folder whose owner and DACL aren't `sddl`'s.
pub(super) fn secure_dir(path: &Path, sddl: &str) -> Result<(), Failed> {
    let descriptor = Descriptor::from_sddl(sddl)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: `name` is NUL-terminated and `attributes` points at a live descriptor; both
    // outlive the call.
    let made = unsafe { CreateDirectoryW(&name, Some(&raw const attributes)) };
    let folder = open_dir(path)?;
    if let Err(error) = made {
        if code_of(&error) != ERROR_ALREADY_EXISTS.0 {
            return Err(failed("CreateDirectoryW", &error));
        }
        set_security(&folder, &descriptor)?;
    }
    let found = sddl_of(&folder)?;
    if !super::sddl_matches(&found, sddl) {
        return Err(format!(
            "the staging folder reads back as {found}, not {sddl}"
        ));
    }
    Ok(())
}

/// The folder `path`, opened for its descriptor without following a reparse point; a reparse
/// point or a file is refused.
fn open_dir(path: &Path) -> Result<Handle, Failed> {
    let folder = open(
        path,
        SECURITY_ACCESS,
        FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0,
    )?;
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle is open and `tag` is valid and writable for its size.
    unsafe {
        GetFileInformationByHandleEx(
            folder.0,
            FileAttributeTagInfo,
            (&raw mut tag).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0),
        )
    }
    .map_err(|error| failed("GetFileInformationByHandleEx", &error))?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err("the staging folder is a reparse point".to_owned());
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
        return Err("the staging folder isn't a folder".to_owned());
    }
    Ok(folder)
}

/// `path` opened with `access`, sharing nothing, with `flags`.
fn open(path: &Path, access: u32, flags: u32) -> Result<Handle, Failed> {
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: `name` is NUL-terminated and outlives the call; the handle is owned at once.
    unsafe {
        CreateFileW(
            &name,
            access,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(flags),
            None,
        )
    }
    .map(Handle)
    .map_err(|error| failed("CreateFileW", &error))
}

/// Sets `descriptor`'s owner and DACL, protected, on the open `object`.
fn set_security(object: &Handle, descriptor: &Descriptor) -> Result<(), Failed> {
    let (owner, dacl) = descriptor.parts()?;
    let what = OWNER_SECURITY_INFORMATION
        | DACL_SECURITY_INFORMATION
        | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: the handle is open with WRITE_DAC and WRITE_OWNER; the owner and DACL point into
    // `descriptor`, which outlives the call.
    let set = unsafe {
        SetSecurityInfo(
            object.0,
            SE_FILE_OBJECT,
            what,
            Some(owner),
            None,
            Some(dacl),
            None,
        )
    };
    if set == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("SetSecurityInfo failed ({})", set.0))
    }
}

/// The open `object`'s owner and DACL, as SDDL.
pub(super) fn sddl_of(object: &Handle) -> Result<String, Failed> {
    let mut descriptor = Descriptor(PSECURITY_DESCRIPTOR(null_mut()));
    let what = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    // SAFETY: the handle is open with READ_CONTROL; the descriptor GetSecurityInfo allocates
    // is freed when `descriptor` drops.
    let read = unsafe {
        GetSecurityInfo(
            object.0,
            SE_FILE_OBJECT,
            what,
            None,
            None,
            None,
            None,
            Some(&raw mut descriptor.0),
        )
    };
    if read != ERROR_SUCCESS {
        return Err(format!("GetSecurityInfo failed ({})", read.0));
    }
    let mut text = PWSTR(null_mut());
    // SAFETY: the descriptor is live; `text` receives a LocalAlloc'd string freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.0,
            SDDL_REVISION_1,
            what,
            &raw mut text,
            None,
        )
    }
    .map_err(|error| {
        failed(
            "ConvertSecurityDescriptorToStringSecurityDescriptorW",
            &error,
        )
    })?;
    // SAFETY: on success the call wrote a NUL-terminated string at `text`.
    let sddl = unsafe { text.to_string() }.unwrap_or_default();
    // SAFETY: `text` came from the conversion's LocalAlloc and is freed once, here.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    Ok(sddl)
}

/// Writes `bytes` as the new file `path`, made with the owner and DACL `sddl` describes, write
/// through and flushed; whatever was at `path` is deleted first.
pub(super) fn write_new(path: &Path, bytes: &[u8], sddl: &str) -> Result<(), Failed> {
    delete(path)?;
    let descriptor = Descriptor::from_sddl(sddl)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: `name` is NUL-terminated and `attributes` points at a live descriptor; both
    // outlive the call. The handle is owned at once.
    let file = unsafe {
        CreateFileW(
            &name,
            GENERIC_WRITE.0,
            FILE_SHARE_MODE(0),
            Some(&raw const attributes),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_WRITE_THROUGH,
            None,
        )
    }
    .map(Handle)
    .map_err(|error| failed("CreateFileW", &error))?;
    let mut rest = bytes;
    while !rest.is_empty() {
        let chunk = rest.get(..rest.len().min(1 << 20)).unwrap_or(rest);
        let mut written = 0u32;
        // SAFETY: the handle is open for writing; `chunk` is live and `written` a live
        // out-pointer. No OVERLAPPED: the handle is synchronous.
        unsafe { WriteFile(file.0, Some(chunk), Some(&raw mut written), None) }
            .map_err(|error| failed("WriteFile", &error))?;
        let written = usize::try_from(written).unwrap_or(0);
        if written == 0 {
            return Err("WriteFile wrote nothing".to_owned());
        }
        rest = rest.get(written..).unwrap_or_default();
    }
    // SAFETY: the handle is open for writing.
    unsafe { FlushFileBuffers(file.0) }.map_err(|error| failed("FlushFileBuffers", &error))
}

/// Gives the file `path` (not followed if a reparse point) the owner and protected DACL `sddl`
/// describes.
pub(super) fn secure_file(path: &Path, sddl: &str) -> Result<(), Failed> {
    let file = open(path, SECURITY_ACCESS, FILE_FLAG_OPEN_REPARSE_POINT.0)?;
    set_security(&file, &Descriptor::from_sddl(sddl)?)
}

/// Renames `from` to `to`, write-through; something at `to` fails it.
pub(super) fn rename(from: &Path, to: &Path) -> Result<(), Failed> {
    let (from, to) = (
        HSTRING::from(from.as_os_str()),
        HSTRING::from(to.as_os_str()),
    );
    // SAFETY: both names are NUL-terminated and outlive the call.
    unsafe { MoveFileExW(&from, &to, MOVEFILE_WRITE_THROUGH) }
        .map_err(|error| failed("MoveFileExW", &error))
}

/// Schedules `path`'s deletion at the next reboot.
pub(super) fn delete_at_reboot(path: &Path) -> Result<(), Failed> {
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: the name is NUL-terminated and outlives the call; no new name.
    unsafe { MoveFileExW(&name, PCWSTR::null(), MOVEFILE_DELAY_UNTIL_REBOOT) }
        .map_err(|error| failed("MoveFileExW", &error))
}

/// Deletes the file `path`; one that isn't there is `Ok`.
pub(super) fn delete(path: &Path) -> Result<(), Failed> {
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: the name is NUL-terminated and outlives the call.
    match unsafe { DeleteFileW(&name) } {
        Ok(()) => Ok(()),
        Err(error) if gone(&error) => Ok(()),
        Err(error) => Err(failed("DeleteFileW", &error)),
    }
}

/// Removes the empty folder `path`; one that isn't there is `Ok`.
pub(super) fn remove_dir(path: &Path) -> Result<(), Failed> {
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: the name is NUL-terminated and outlives the call.
    match unsafe { RemoveDirectoryW(&name) } {
        Ok(()) => Ok(()),
        Err(error) if gone(&error) => Ok(()),
        Err(error) => Err(failed("RemoveDirectoryW", &error)),
    }
}

/// Whether `error` says the file or its folder isn't there.
fn gone(error: &windows::core::Error) -> bool {
    let code = code_of(error);
    code == ERROR_FILE_NOT_FOUND.0 || code == ERROR_PATH_NOT_FOUND.0
}

#[cfg(test)]
mod tests;

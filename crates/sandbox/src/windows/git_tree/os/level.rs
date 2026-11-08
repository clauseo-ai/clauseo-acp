//! A Git tree's folders made and secured ([`level`]): opened, or made, relative to their
//! parent's handle (`NtCreateFile`, `FILE_OPEN_IF`, `OBJ_DONT_REPARSE`, `FILE_OPEN_REPARSE_POINT`)
//! so no link on the way or at the name is followed, then given their owner and protected DACL
//! through that handle and read back.
//!
//! The descriptor is set with `SetKernelObjectSecurity`, on the folder alone: an account's
//! folder holds trees whose files are hard links, sharing their security with Git's own files,
//! and `SetSecurityInfo`'s propagation would rewrite those. The trees' files keep Git's DACL;
//! what reaches a pool user is the folders'.
#![expect(
    unsafe_code,
    reason = "FFI to NtCreateFile and the security descriptor calls"
)]

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::ptr::null_mut;

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_OPEN_FOR_BACKUP_INTENT, FILE_OPEN_IF, FILE_OPEN_REPARSE_POINT,
    FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
};
use windows::Win32::Foundation::{
    HANDLE, HLOCAL, LocalFree, NTSTATUS, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
    RtlNtStatusToDosError, UNICODE_STRING,
};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SetKernelObjectSecurity,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ACCESS_RIGHTS, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_FLAGS_AND_ATTRIBUTES, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo, READ_CONTROL, SYNCHRONIZE, WRITE_DAC,
    WRITE_OWNER,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::super::Refused;
use super::files::info;
use crate::windows::acl::object::{Handle, Object, kernel_security, reparse_refused, words};
use crate::windows::acl::privilege::Privileges;
use crate::windows::admin_copy::{Found, os as admin_os};
use crate::windows::refresh::sddl_matches;

/// What is read back and compared: the owner and the DACL.
const READ_BACK: OBJECT_SECURITY_INFORMATION =
    OBJECT_SECURITY_INFORMATION(OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0);

/// `ERROR_DIRECTORY`: a file where a folder was asked for.
const NOT_A_FOLDER: i32 = 267;

/// Makes the folder `path` if missing, opened as itself relative to its parent's handle, then
/// gives it the owner and protected DACL `sddl` describes and reads them back. A reparse point
/// at the parent or at `path` is [`Refused::Reparse`]; a file there is refused. Setting another
/// owner than the caller (SYSTEM, as an elevated administrator) needs `SeRestorePrivilege`,
/// enabled for the call, which also opens a folder whose DACL leaves Administrators out.
pub(super) fn level(path: &Path, sddl: &str) -> Result<(), Refused> {
    let fail = |why: &str| Refused::Other(format!("{}: {why}", path.display()));
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(fail("has no parent"));
    };
    let _restore = Privileges::enable(&["SeRestorePrivilege"])
        .map_err(|error| fail(&format!("enabling SeRestorePrivilege: {error}")))?;
    let above = Object::open_folder(parent).map_err(|error| match error {
        e if reparse_refused(&e) => Refused::Reparse,
        e => fail(&format!("opening its parent: {e}")),
    })?;
    if above.reparse() {
        return Err(Refused::Reparse);
    }
    if !above.folder {
        return Err(fail("its parent isn't a folder"));
    }
    let name: Vec<u16> = name.encode_wide().collect();
    let folder = match open_or_make(above.handle(), &name) {
        Ok(folder) => folder,
        Err(error) if reparse_refused(&error) => return Err(Refused::Reparse),
        Err(error) if error.raw_os_error() == Some(NOT_A_FOLDER) => {
            // A file, or a link to one, opened as itself.
            return Err(match admin_os::found(path) {
                Ok(Found::Link) => Refused::Reparse,
                _ => fail("is a file"),
            });
        }
        Err(error) => return Err(fail(&format!("opening it: {error}"))),
    };
    let tag: FILE_ATTRIBUTE_TAG_INFO =
        info(&folder, FileAttributeTagInfo).map_err(|error| fail(&error.to_string()))?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Refused::Reparse);
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
        return Err(fail("isn't a folder"));
    }
    secure(&folder, sddl).map_err(|why| fail(&why))
}

/// Sets `sddl`'s owner and protected DACL on what `folder` is open on, then reads them back
/// and refuses unless they are that (`refresh::sddl_matches`).
fn secure(folder: &Handle, sddl: &str) -> Result<(), String> {
    let wanted = Descriptor::from_sddl(sddl)?;
    let info = READ_BACK | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: the handle is open with WRITE_DAC and WRITE_OWNER; the descriptor is a valid,
    // self-relative one, alive for the call.
    unsafe { SetKernelObjectSecurity(folder.0, info, wanted.0) }
        .map_err(|error| format!("SetKernelObjectSecurity failed ({})", code(&error)))?;
    let back = kernel_security(folder, READ_BACK.0)
        .map_err(|error| format!("reading the security back: {error}"))?;
    let mut back = words(&back);
    let found = to_sddl(PSECURITY_DESCRIPTOR(back.as_mut_ptr().cast()))?;
    if !sddl_matches(&found, &wanted.to_sddl()?) {
        // Neither descriptor is given: either names a SID.
        return Err("the folder's security reads back changed".to_owned());
    }
    Ok(())
}

/// Opens the folder `name` in the folder `dir` relative to its handle, making it when missing
/// (inheriting `dir`'s DACL until [`secure`] replaces it), with what [`secure`] needs. A
/// reparse point at `name` is opened itself; one met on the way fails the open.
fn open_or_make(dir: &Handle, name: &[u16]) -> io::Result<Handle> {
    let bytes = u16::try_from(name.len() * 2)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let unicode = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(name.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
        RootDirectory: dir.0,
        ObjectName: &raw const unicode,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..OBJECT_ATTRIBUTES::default()
    };
    let access = READ_CONTROL.0 | WRITE_DAC.0 | WRITE_OWNER.0 | FILE_READ_ATTRIBUTES.0;
    let options = FILE_DIRECTORY_FILE
        | FILE_OPEN_REPARSE_POINT
        | FILE_OPEN_FOR_BACKUP_INTENT
        | FILE_SYNCHRONOUS_IO_NONALERT;
    let mut handle = HANDLE::default();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: `unicode` points into `name`, which outlives the call and is never written;
    // `attributes` and the out-pointers are live; the folder's handle is open; the handle made
    // is owned by `Handle` from here.
    let done = unsafe {
        NtCreateFile(
            &raw mut handle,
            FILE_ACCESS_RIGHTS(access | SYNCHRONIZE.0),
            &raw const attributes,
            &raw mut status,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN_IF,
            options,
            None,
            0,
        )
    };
    if done.is_err() {
        return Err(nt_error(done));
    }
    Ok(Handle(handle))
}

/// The NT status `status`, as the Win32 error it maps to.
fn nt_error(status: NTSTATUS) -> io::Error {
    // SAFETY: RtlNtStatusToDosError only maps a number.
    let code = unsafe { RtlNtStatusToDosError(status) };
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}

/// The Win32 code `error` carries.
fn code(error: &windows::core::Error) -> i32 {
    error.code().0 & 0xFFFF
}

/// The owner and DACL of the descriptor `descriptor` as SDDL, as Windows writes it.
fn to_sddl(descriptor: PSECURITY_DESCRIPTOR) -> Result<String, String> {
    let mut text = PWSTR(null_mut());
    // SAFETY: the caller's descriptor is valid for the call; `text` receives a LocalAlloc'd
    // string, freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            READ_BACK,
            &raw mut text,
            None,
        )
    }
    .map_err(|error| format!("writing a descriptor as SDDL failed ({})", code(&error)))?;
    // SAFETY: on success the call wrote a NUL-terminated string at `text`.
    let sddl = unsafe { text.to_string() }.unwrap_or_default();
    // SAFETY: `text` came from the conversion's LocalAlloc and is freed once, here.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    Ok(sddl)
}

/// A self-relative security descriptor from SDDL, allocated with `LocalAlloc`, freed on drop.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// The descriptor `sddl` describes.
    fn from_sddl(sddl: &str) -> Result<Descriptor, String> {
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
        .map_err(|error| format!("reading a folder's SDDL failed ({})", code(&error)))?;
        Ok(Descriptor(descriptor))
    }

    /// Its owner and DACL as SDDL, as Windows writes them back (`SY`, `BA`, `S-1-...`).
    fn to_sddl(&self) -> Result<String, String> {
        to_sddl(self.0)
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: allocated with LocalAlloc by the conversion and freed here, once.
            unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }
}

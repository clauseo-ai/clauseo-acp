//! The NT calls the Git trees' files share: a file or folder opened or made relative to its
//! folder's handle ([`Open::at`]: `NtCreateFile`, `OBJ_DONT_REPARSE`, `FILE_OPEN_REPARSE_POINT`,
//! backup intent), a descriptor given at creation ([`Descriptor`], from SDDL), and
//! `NtSetInformationFile` ([`set_information`]). Nothing here sets the security of what is
//! already there: a new file's descriptor is given in its `OBJECT_ATTRIBUTES`.
#![expect(
    unsafe_code,
    reason = "FFI to NtCreateFile, NtSetInformationFile and the SDDL conversions"
)]

use std::io;
use std::ptr::null_mut;

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_INFORMATION_CLASS, FILE_OPEN_FOR_BACKUP_INTENT, FILE_OPEN_REPARSE_POINT,
    FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_DISPOSITION, NTCREATEFILE_CREATE_OPTIONS,
    NtCreateFile, NtSetInformationFile,
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
    PSECURITY_DESCRIPTOR, SECURITY_DESCRIPTOR,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ACCESS_RIGHTS, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE, SYNCHRONIZE,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::core::{HSTRING, PCWSTR, PWSTR};

use crate::windows::acl::object::Handle;

/// The owner and the DACL: what [`to_sddl`] writes.
pub(super) const OWNER_DACL: OBJECT_SECURITY_INFORMATION =
    OBJECT_SECURITY_INFORMATION(OWNER_SECURITY_INFORMATION.0 | DACL_SECURITY_INFORMATION.0);

/// A descriptor naming SYSTEM as owner and no DACL: what is made with it inherits its folder's.
pub(super) const OWNER_SYSTEM: &str = "O:SY";

/// How a name is opened or made relative to its folder's handle. Every open asks for
/// `SYNCHRONIZE` and adds `FILE_OPEN_REPARSE_POINT` (a reparse point at the name is opened
/// itself), `FILE_OPEN_FOR_BACKUP_INTENT` (an enabled backup or restore privilege counts) and
/// `FILE_SYNCHRONOUS_IO_NONALERT`; `OBJ_DONT_REPARSE` fails it at a reparse point on the way.
pub(super) struct Open {
    /// The rights asked for, `SYNCHRONIZE` aside.
    pub(super) access: u32,
    /// What others may do while it is open.
    pub(super) share: FILE_SHARE_MODE,
    /// `FILE_OPEN`, `FILE_CREATE` or `FILE_OPEN_IF`.
    pub(super) disposition: NTCREATEFILE_CREATE_DISPOSITION,
    /// `FILE_DIRECTORY_FILE`, `FILE_NON_DIRECTORY_FILE` and the like.
    pub(super) options: NTCREATEFILE_CREATE_OPTIONS,
}

impl Open {
    /// Opens or makes `name` (one path part, no NUL) in the folder `dir`, relative to its
    /// handle; what is made gets `descriptor`, else its folder's inherited security.
    pub(super) fn at(
        &self,
        dir: &Handle,
        name: &[u16],
        descriptor: Option<&Descriptor>,
    ) -> io::Result<Handle> {
        let bytes = u16::try_from(name.len() * 2)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let unicode = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: PWSTR(name.as_ptr().cast_mut()),
        };
        let security = descriptor.map_or(std::ptr::null(), |held| {
            held.0.0.cast_const().cast::<SECURITY_DESCRIPTOR>()
        });
        let attributes = OBJECT_ATTRIBUTES {
            Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
            RootDirectory: dir.0,
            ObjectName: &raw const unicode,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            SecurityDescriptor: security,
            ..OBJECT_ATTRIBUTES::default()
        };
        let options = self.options
            | FILE_OPEN_REPARSE_POINT
            | FILE_OPEN_FOR_BACKUP_INTENT
            | FILE_SYNCHRONOUS_IO_NONALERT;
        let mut handle = HANDLE::default();
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: `unicode` points into `name`, which outlives the call and is never written;
        // the descriptor, when given, is a valid self-relative one alive for the call;
        // `attributes` and the out-pointers are live; the folder's handle is open; the handle
        // made is owned by `Handle` from here.
        let done = unsafe {
            NtCreateFile(
                &raw mut handle,
                FILE_ACCESS_RIGHTS(self.access | SYNCHRONIZE.0),
                &raw const attributes,
                &raw mut status,
                None,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                self.share,
                self.disposition,
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
}

/// Sets the `class` information of what `handle` is open on from `data`, which holds that
/// class's struct (variable-length ones included) and is 8-byte aligned.
pub(super) fn set_information(
    handle: &Handle,
    class: FILE_INFORMATION_CLASS,
    data: &[u64],
    length: usize,
) -> io::Result<()> {
    if length > size_of_val(data) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "information longer than its buffer",
        ));
    }
    let length = u32::try_from(length).map_err(io::Error::other)?;
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: the handle is open; `data` is live, aligned and at least `length` bytes, holding
    // the struct `class` reads; `status` is a live out-pointer.
    let done = unsafe {
        NtSetInformationFile(
            handle.0,
            &raw mut status,
            data.as_ptr().cast(),
            length,
            class,
        )
    };
    if done.is_err() {
        return Err(nt_error(done));
    }
    Ok(())
}

/// The NT status `status`, as the Win32 error it maps to.
pub(super) fn nt_error(status: NTSTATUS) -> io::Error {
    // SAFETY: RtlNtStatusToDosError only maps a number.
    let code = unsafe { RtlNtStatusToDosError(status) };
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}

/// The Win32 code `error` carries.
pub(super) fn code(error: &windows::core::Error) -> i32 {
    error.code().0 & 0xFFFF
}

/// The owner and DACL of the descriptor `descriptor` as SDDL, as Windows writes it.
pub(super) fn to_sddl(descriptor: PSECURITY_DESCRIPTOR) -> Result<String, String> {
    let mut text = PWSTR(null_mut());
    // SAFETY: the caller's descriptor is valid for the call; `text` receives a LocalAlloc'd
    // string, freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            OWNER_DACL,
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
pub(super) struct Descriptor(pub(super) PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// The descriptor `sddl` describes.
    pub(super) fn from_sddl(sddl: &str) -> Result<Descriptor, String> {
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
        .map_err(|error| format!("reading an SDDL descriptor failed ({})", code(&error)))?;
        Ok(Descriptor(descriptor))
    }

    /// Its owner and DACL as SDDL, as Windows writes them back (`SY`, `BA`, `S-1-...`).
    pub(super) fn to_sddl(&self) -> Result<String, String> {
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

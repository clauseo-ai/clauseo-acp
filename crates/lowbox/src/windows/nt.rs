//! The native calls the `windows` crate does not declare, and the object attributes and
//! security descriptors the namespace calls take.
#![expect(
    unsafe_code,
    reason = "FFI to ntdll, and the SDDL conversion's LocalFree"
)]

use std::ffi::c_void;

use ::windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use ::windows::Win32::Foundation::{
    HANDLE, HLOCAL, LocalFree, NTSTATUS, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING,
};
use ::windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use ::windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES,
};
use ::windows::core::{PCWSTR, PWSTR};

use super::{wide_nul, win32};
use crate::error::Error;

::windows::core::link!("ntdll.dll" "system" fn NtCreateDirectoryObjectEx(
    handle: *mut HANDLE,
    access: u32,
    attributes: *const OBJECT_ATTRIBUTES,
    shadow: HANDLE,
    flags: u32
) -> NTSTATUS);

::windows::core::link!("ntdll.dll" "system" fn NtCreateSymbolicLinkObject(
    handle: *mut HANDLE,
    access: u32,
    attributes: *const OBJECT_ATTRIBUTES,
    target: *const UNICODE_STRING
) -> NTSTATUS);

::windows::core::link!("ntdll.dll" "system" fn NtCreateLowBoxToken(
    token: *mut HANDLE,
    existing: HANDLE,
    access: u32,
    attributes: *const OBJECT_ATTRIBUTES,
    package: PSID,
    capability_count: u32,
    capabilities: *const SID_AND_ATTRIBUTES,
    handle_count: u32,
    handles: *const HANDLE
) -> NTSTATUS);

/// A kernel object name as `UNICODE_STRING`, with the text it points into.
pub(super) struct ObjectName {
    /// The UTF-16 text; the string points into its heap buffer, which never moves.
    _units: Vec<u16>,
    /// The counted string.
    string: UNICODE_STRING,
}

impl ObjectName {
    /// The counted string of `text`, which must fit 32 766 units.
    pub(super) fn new(text: &str) -> Result<ObjectName, Error> {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        let bytes = u16::try_from(units.len() * 2)
            .ok()
            .filter(|b| *b < u16::MAX - 1)
            .ok_or(Error::Win32 {
                call: "an object name over 32 766 characters",
                // The Win32 error for a name past its range (206).
                code: 206,
            })?;
        let string = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: PWSTR(units.as_mut_ptr()),
        };
        Ok(ObjectName {
            _units: units,
            string,
        })
    }

    /// The counted string, valid while `self` lives.
    pub(super) fn as_ptr(&self) -> *const UNICODE_STRING {
        &raw const self.string
    }

    /// Case-insensitive object attributes for this name, with `descriptor` when given. Valid
    /// while `self` and `descriptor` live.
    pub(super) fn attributes(&self, descriptor: Option<&Descriptor>) -> OBJECT_ATTRIBUTES {
        OBJECT_ATTRIBUTES {
            Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(u32::MAX),
            RootDirectory: HANDLE::default(),
            ObjectName: self.as_ptr(),
            Attributes: OBJ_CASE_INSENSITIVE,
            SecurityDescriptor: descriptor.map_or(std::ptr::null(), |d| d.0.0.cast()),
            SecurityQualityOfService: std::ptr::null(),
        }
    }

    /// As [`ObjectName::attributes`], with `OBJ_DONT_REPARSE`: the open fails rather than follow
    /// an object-manager symbolic link anywhere on the path.
    pub(super) fn attributes_no_reparse(
        &self,
        descriptor: Option<&Descriptor>,
    ) -> OBJECT_ATTRIBUTES {
        let mut attributes = self.attributes(descriptor);
        attributes.Attributes = OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE;
        attributes
    }
}

/// A self-relative security descriptor from SDDL, freed with `LocalFree`.
pub(super) struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// The descriptor `sddl` describes.
    pub(super) fn from_sddl(sddl: &str) -> Result<Descriptor, Error> {
        let text = wide_nul(sddl.as_ref());
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` receives a
        // LocalAlloc'd pointer, freed once by Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(|e| win32("ConvertStringSecurityDescriptorToSecurityDescriptorW", &e))?;
        Ok(Descriptor(descriptor))
    }

    /// This descriptor's DACL (`D:…`) and mandatory label (`S:…`) as SDDL, through
    /// [`dacl_and_label`], so they read exactly as the ones read back from an object do.
    pub(super) fn dacl_and_label(&self) -> Result<(String, String), Error> {
        // SAFETY: `self.0` is a valid self-relative descriptor until Drop.
        unsafe { dacl_and_label(self.0) }.map_err(|code| Error::Win32 {
            call: "ConvertSecurityDescriptorToStringSecurityDescriptorW",
            code,
        })
    }
}

/// The DACL (`D:…`) and the mandatory label (`S:…`) of `descriptor` as SDDL, each converted
/// on its own, else the Win32 error.
///
/// # Safety
/// `descriptor` must point at a valid security descriptor for the call.
pub(super) unsafe fn dacl_and_label(
    descriptor: PSECURITY_DESCRIPTOR,
) -> Result<(String, String), u32> {
    // SAFETY: the caller's contract, passed on.
    let dacl = unsafe { sddl_part(descriptor, DACL_SECURITY_INFORMATION) }?;
    // SAFETY: as above.
    let label = unsafe { sddl_part(descriptor, LABEL_SECURITY_INFORMATION) }?;
    Ok((dacl, label))
}

/// The part `info` of `descriptor` as SDDL, else the Win32 error.
///
/// # Safety
/// `descriptor` must point at a valid security descriptor for the call.
unsafe fn sddl_part(
    descriptor: PSECURITY_DESCRIPTOR,
    info: OBJECT_SECURITY_INFORMATION,
) -> Result<String, u32> {
    let mut text = PWSTR::null();
    // SAFETY: `descriptor` is valid (the caller's contract); `text` gets a LocalAlloc'd string,
    // freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            info,
            &raw mut text,
            None,
        )
    }
    .map_err(|e| e.code().0.cast_unsigned() & 0xFFFF)?;
    // SAFETY: the call succeeded, so `text` is a NUL-terminated string.
    let string = unsafe { text.to_string() };
    // SAFETY: `text` came from the conversion (LocalAlloc) and is freed once, here; a failure
    // to free leaks a few bytes, nothing to undo.
    let _leak_on_failure: HLOCAL = unsafe { LocalFree(Some(HLOCAL(text.0.cast::<c_void>()))) };
    // ERROR_INVALID_DATA (13): text that is not UTF-16.
    string.map_err(|_not_utf16| 13)
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the pointer came from ConvertStringSecurityDescriptorToSecurityDescriptorW,
        // which allocates with LocalAlloc, and is freed only here. LocalFree returns null on
        // success; a failure leaves a small leak, nothing to undo.
        let _leak_on_failure: HLOCAL =
            unsafe { LocalFree(Some(HLOCAL(self.0.0.cast::<c_void>()))) };
    }
}

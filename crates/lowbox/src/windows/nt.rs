//! The native calls the `windows` crate does not declare, and the object attributes and
//! security descriptors the namespace calls take.
#![expect(
    unsafe_code,
    reason = "FFI to ntdll, and the SDDL conversion's LocalFree"
)]

use std::ffi::c_void;

use ::windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use ::windows::Win32::Foundation::{
    HANDLE, HLOCAL, LocalFree, NTSTATUS, OBJ_CASE_INSENSITIVE, UNICODE_STRING,
};
use ::windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use ::windows::Win32::Security::{PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES};
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

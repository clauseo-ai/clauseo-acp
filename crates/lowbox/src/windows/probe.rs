//! The real [`Probe`]: `GetStdHandle`, `GetHandleInformation`, `NtQueryObject` and the owner
//! (`GetSecurityInfo`) on this process's handles.
#![expect(
    unsafe_code,
    reason = "FFI to GetStdHandle, GetHandleInformation and NtQueryObject"
)]

use std::ffi::c_void;

use ::windows::Wdk::Foundation::{NtQueryObject, OBJECT_INFORMATION_CLASS, ObjectTypeInformation};
use ::windows::Win32::Foundation::{GetHandleInformation, HANDLE, UNICODE_STRING};

use ::windows::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};

use super::{token, win32};
use crate::error::Error;
use crate::handles::Probe;

/// `ObjectNameInformation`.
const OBJECT_NAME_INFORMATION: OBJECT_INFORMATION_CLASS = OBJECT_INFORMATION_CLASS(1);
/// `STATUS_INFO_LENGTH_MISMATCH`, `STATUS_BUFFER_OVERFLOW` and `STATUS_BUFFER_TOO_SMALL`: the
/// buffer was short and the returned length says how long it must be.
const SHORT: [u32; 3] = [0xC000_0004, 0x8000_0005, 0xC000_0023];
/// The first buffer, in 8-byte units: room for a type name or a namespace path.
const FIRST_UNITS: usize = 256;
/// The largest buffer tried, in 8-byte units (64 KiB, past the longest object name).
const MAX_UNITS: usize = 8192;

/// The handles of this process.
pub(super) struct ThisProcess;

impl Probe for ThisProcess {
    fn std_handles(&self) -> [usize; 3] {
        [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE].map(|id| {
            // SAFETY: GetStdHandle only reads this process's parameters.
            unsafe { GetStdHandle(id) }.map_or(0, |h| h.0.expose_provenance())
        })
    }

    fn check_open(&self, value: usize) -> Result<(), u32> {
        let mut flags = 0_u32;
        // SAFETY: GetHandleInformation only reads the handle table entry of `value`; a value
        // that is no handle makes it fail with ERROR_INVALID_HANDLE.
        unsafe { GetHandleInformation(handle(value), &raw mut flags) }.map_err(|e| {
            match win32("GetHandleInformation", &e) {
                Error::Win32 { code, .. } => code,
                _ => u32::MAX,
            }
        })
    }

    fn type_name(&self, value: usize) -> Result<String, i32> {
        query_string(value, ObjectTypeInformation)
    }

    fn object_name(&self, value: usize) -> Result<String, i32> {
        query_string(value, OBJECT_NAME_INFORMATION)
    }

    fn owner(&self, value: usize) -> Result<String, u32> {
        token::owner_of(handle(value))
    }
}

/// The handle with the value `value`.
pub(super) fn handle(value: usize) -> HANDLE {
    HANDLE(std::ptr::with_exposed_provenance_mut(value))
}

/// The `UNICODE_STRING` that `NtQueryObject` puts first for `class` (the type name for
/// `ObjectTypeInformation`, the name for `ObjectNameInformation`), as text.
fn query_string(value: usize, class: OBJECT_INFORMATION_CLASS) -> Result<String, i32> {
    let mut units = FIRST_UNITS;
    loop {
        // u64 units keep the buffer 8-byte aligned, as the UNICODE_STRING in it must be.
        let mut buffer = vec![0_u64; units];
        let bytes = u32::try_from(units * 8).unwrap_or(u32::MAX);
        let mut needed = 0_u32;
        // SAFETY: `buffer` is writable for `bytes` bytes and outlives the call; NtQueryObject
        // writes at most that many and the needed length into `needed`.
        let status = unsafe {
            NtQueryObject(
                Some(handle(value)),
                class,
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                bytes,
                Some(&raw mut needed),
            )
        };
        if status.0 >= 0 {
            return Ok(read_string(&buffer));
        }
        let next = (needed as usize).div_ceil(8);
        if !SHORT.contains(&status.0.cast_unsigned()) || next <= units || next > MAX_UNITS {
            return Err(status.0);
        }
        units = next;
    }
}

/// The text of the `UNICODE_STRING` at the start of `buffer`, where `NtQueryObject` points its
/// characters into the same buffer.
fn read_string(buffer: &[u64]) -> String {
    // SAFETY: NtQueryObject succeeded, so the buffer starts with a UNICODE_STRING (the buffer
    // is 8-byte aligned and longer than one); read_unaligned copies it out without a reference.
    let string = unsafe { buffer.as_ptr().cast::<UNICODE_STRING>().read_unaligned() };
    let len = usize::from(string.Length / 2);
    if len == 0 || string.Buffer.is_null() {
        return String::new();
    }
    let start = buffer.as_ptr().cast::<u8>();
    let end = start.wrapping_add(buffer.len() * 8);
    let chars = string.Buffer.0.cast_const().cast::<u8>();
    if chars < start || chars.wrapping_add(len * 2) > end {
        // Never seen: the kernel puts the text in the caller's buffer. Read nothing outside it.
        return String::new();
    }
    // SAFETY: the text lies inside `buffer` (checked above), which outlives the slice, and
    // holds `len` UTF-16 units written by the kernel.
    let text = unsafe { std::slice::from_raw_parts(string.Buffer.0.cast_const(), len) };
    String::from_utf16_lossy(text)
}

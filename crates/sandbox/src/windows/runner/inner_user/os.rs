//! The inner runner's user read without its token (Windows):
//! `NtQueryInformationProcess(ProcessTelemetryIdInformation)` through the process handle the
//! logon gave.
#![expect(unsafe_code, reason = "FFI to NtQueryInformationProcess")]

use std::ffi::c_void;

use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows::Win32::Foundation::{
    STATUS_BUFFER_OVERFLOW, STATUS_BUFFER_TOO_SMALL, STATUS_INFO_LENGTH_MISMATCH,
};
use windows::Win32::Security::{IsValidSid, PSID};

use super::super::spawn::Process;
use super::{PROCESS_TELEMETRY_ID_INFORMATION, telemetry_sid};
use crate::windows::sids::Sid;

/// The telemetry buffer's first size (the VM's was 456 bytes); it grows to what the call asks,
/// up to [`TELEMETRY_MAX`].
const TELEMETRY_FIRST: usize = 4096;
/// The largest telemetry buffer: a header, a SID and four paths and names.
const TELEMETRY_MAX: usize = 256 * 1024;

/// `inner`'s user from `ProcessTelemetryIdInformation`, which needs only
/// `PROCESS_QUERY_LIMITED_INFORMATION` on the process handle (the logon's has every right).
///
/// The pid the buffer must name is stable: this process holds the creation handle
/// `CreateProcessWithLogonW` gave, so the process object, and its pid, can't be freed and
/// reused while it is held, and the inner is still suspended, so it can't have changed its own
/// token yet either.
pub(in crate::windows::runner) fn telemetry(inner: &Process) -> Result<Sid, String> {
    let mut size = TELEMETRY_FIRST;
    loop {
        let mut buffer = vec![0u8; size];
        let len = u32::try_from(size).unwrap_or(u32::MAX);
        let mut needed = 0u32;
        // SAFETY: the process handle is live; `buffer` is a live, writable `len`-byte buffer;
        // `needed` is a live out-pointer.
        let status = unsafe {
            NtQueryInformationProcess(
                inner.handle.0,
                PROCESSINFOCLASS(PROCESS_TELEMETRY_ID_INFORMATION),
                buffer.as_mut_ptr().cast::<c_void>(),
                len,
                &raw mut needed,
            )
        };
        let short = status == STATUS_INFO_LENGTH_MISMATCH
            || status == STATUS_BUFFER_TOO_SMALL
            || status == STATUS_BUFFER_OVERFLOW;
        let wanted = usize::try_from(needed).unwrap_or(usize::MAX);
        if short && wanted > size && wanted <= TELEMETRY_MAX {
            size = wanted;
            continue;
        }
        if status.is_err() {
            return Err(format!(
                "NtQueryInformationProcess(ProcessTelemetryIdInformation) failed (0x{:08X})",
                status.0.cast_unsigned()
            ));
        }
        // Only the bytes written are read; a length past the buffer leaves it whole, and the
        // parse checks every offset against what is left.
        buffer.truncate(wanted);
        let (offset, sid) = telemetry_sid(&buffer, inner.pid)?;
        // `telemetry_sid` checked the SID's bytes lie inside `buffer` from `offset`.
        let at = buffer
            .get(offset..)
            .ok_or_else(|| "ProcessTelemetryIdInformation: the user's offset".to_owned())?;
        // SAFETY: `at` is a live slice holding a whole SID (its count's sub-authorities
        // included), which IsValidSid only reads.
        if !unsafe { IsValidSid(PSID(at.as_ptr().cast_mut().cast())) }.as_bool() {
            return Err(
                "ProcessTelemetryIdInformation: IsValidSid refused the user's SID".to_owned(),
            );
        }
        return Ok(sid);
    }
}

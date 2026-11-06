//! The set-up's preflight on Windows (S15 `PR4f`, step 2): how the Secondary Logon service
//! starts (`QueryServiceConfigW` on `seclogon`), and whether "Deny log on locally" names a
//! group every pool user is in (`LsaEnumerateAccountsWithUserRight`). Both answer before any
//! account exists; "Allow log on locally" is tested by the logon itself (step 6), where
//! `ERROR_LOGON_TYPE_NOT_GRANTED` (1385) gets its named cause (D-B4). Also this machine's
//! account domain, which the process stop and the profile delete check pool SIDs against.
#![expect(unsafe_code, reason = "FFI to the service manager and the LSA policy")]

use std::ffi::c_void;

use windows::Win32::Foundation::{
    ERROR_INSUFFICIENT_BUFFER, ERROR_NONE_MAPPED, ERROR_SERVICE_DOES_NOT_EXIST, HLOCAL, LocalFree,
    NTSTATUS, STATUS_NO_MORE_ENTRIES,
};
use windows::Win32::Security::Authentication::Identity::{
    LSA_ENUMERATION_INFORMATION, LSA_HANDLE, LSA_OBJECT_ATTRIBUTES, LSA_UNICODE_STRING, LsaClose,
    LsaEnumerateAccountsWithUserRight, LsaFreeMemory, LsaNtStatusToWinError, LsaOpenPolicy,
    LsaQueryInformationPolicy, POLICY_ACCOUNT_DOMAIN_INFO, POLICY_LOOKUP_NAMES,
    POLICY_VIEW_LOCAL_INFORMATION, PolicyAccountDomainInformation,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{LookupAccountNameW, PSID, SID_NAME_USE, SidTypeAlias};
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfigW,
    SC_HANDLE, SC_MANAGER_CONNECT, SERVICE_QUERY_CONFIG,
};
use windows::core::{PCWSTR, PWSTR, w};

use super::answers::{DENY_INTERACTIVE_RIGHT, logon_right_of, service_start};
use super::ffi::{os, wide};
use super::machine::{LogonRight, MachineError, ServiceStart};
use crate::windows::pool::POOL_GROUP;
use crate::windows::sids::Sid;

/// How the Secondary Logon service (`seclogon`) starts. A machine without the service answers
/// [`ServiceStart::Disabled`]: no pool user can be logged on through it there either.
pub fn seclogon() -> Result<ServiceStart, MachineError> {
    // SAFETY: no machine or database name (the local one); the handle is owned by `Service`.
    let manager = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map(Service)
        .map_err(|error| os("OpenSCManagerW", &error))?;
    // SAFETY: `manager` is open; the name is a NUL-terminated literal; the handle is owned by
    // `Service`.
    let opened = unsafe { OpenServiceW(manager.0, w!("seclogon"), SERVICE_QUERY_CONFIG) };
    let service = match opened {
        Ok(handle) => Service(handle),
        Err(error) if error.code() == ERROR_SERVICE_DOES_NOT_EXIST.to_hresult() => {
            return Ok(ServiceStart::Disabled);
        }
        Err(error) => return Err(os("OpenServiceW", &error)),
    };
    let mut needed = 0u32;
    // SAFETY: no buffer and a zero length ask only for the size, written to `needed`.
    match unsafe { QueryServiceConfigW(service.0, None, 0, &raw mut needed) } {
        Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {}
        Err(error) => return Err(os("QueryServiceConfigW", &error)),
        Ok(()) => {
            return Err(MachineError::Other(
                "seclogon's configuration is empty".into(),
            ));
        }
    }
    let bytes = usize::try_from(needed)
        .unwrap_or(0)
        .max(size_of::<QUERY_SERVICE_CONFIGW>());
    // u64 words: the structure holds pointers, so the buffer is aligned for it.
    let mut buffer = vec![0u64; bytes.div_ceil(size_of::<u64>())];
    let len = u32::try_from(size_of_val(buffer.as_slice())).unwrap_or(u32::MAX);
    let config: *mut QUERY_SERVICE_CONFIGW = buffer.as_mut_ptr().cast();
    // SAFETY: `config` points into `buffer`, which is live, aligned and `len` bytes long.
    unsafe { QueryServiceConfigW(service.0, Some(config), len, &raw mut needed) }
        .map_err(|error| os("QueryServiceConfigW", &error))?;
    // SAFETY: the call succeeded, so `buffer` starts with a QUERY_SERVICE_CONFIGW.
    let start_type = unsafe { (*config).dwStartType.0 };
    drop(service);
    drop(manager);
    service_start(start_type).ok_or_else(|| {
        MachineError::Other(format!("seclogon has an unknown start type ({start_type})"))
    })
}

/// Whether "Deny log on locally" refuses the pool users ([`logon_right_of`]), counting the
/// pool group's SID when the group exists. Needs an administrator: the right's holders are
/// not readable by a standard user.
pub fn logon_right() -> Result<LogonRight, MachineError> {
    let access = POLICY_LOOKUP_NAMES | POLICY_VIEW_LOCAL_INFORMATION;
    let policy = open_policy(access)?;
    let mut right: Vec<u16> = DENY_INTERACTIVE_RIGHT.encode_utf16().collect();
    let length = u16::try_from(size_of_val(right.as_slice())).unwrap_or(0);
    let name = LSA_UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: PWSTR(right.as_mut_ptr()),
    };
    let mut buffer: *mut c_void = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: `policy` is open; `name` points into `right`, which outlives the call; `buffer`
    // and `count` are live out-pointers, and the buffer is freed by `LsaBuffer`.
    let status = unsafe {
        LsaEnumerateAccountsWithUserRight(
            policy.0,
            Some(&raw const name),
            &raw mut buffer,
            &raw mut count,
        )
    };
    if status == STATUS_NO_MORE_ENTRIES {
        return Ok(LogonRight::Allowed);
    }
    lsa_check("LsaEnumerateAccountsWithUserRight", status)?;
    let buffer = LsaBuffer(buffer);
    let entries: &[LSA_ENUMERATION_INFORMATION] = if buffer.0.is_null() {
        &[]
    } else {
        // SAFETY: the call succeeded and wrote `count` entries at `buffer`, which lives until
        // `LsaBuffer` drops at the end of this function.
        unsafe {
            std::slice::from_raw_parts(
                buffer.0.cast::<LSA_ENUMERATION_INFORMATION>(),
                usize::try_from(count).unwrap_or(0),
            )
        }
    };
    let mut denied = Vec::with_capacity(entries.len());
    for entry in entries {
        // SAFETY: `entry.Sid` is a valid SID inside the live buffer.
        if let Some(sid) = unsafe { sid_text(entry.Sid) }? {
            denied.push(sid);
        }
    }
    drop(buffer);
    drop(policy);
    Ok(logon_right_of(&denied, group_sid()?.as_ref()))
}

/// This machine's account domain: the three parts after `S-1-5-21` that every local account's
/// SID starts with (`LsaQueryInformationPolicy`, `PolicyAccountDomainInformation`).
pub(super) fn machine_domain() -> Result<[u32; 3], MachineError> {
    let policy = open_policy(POLICY_VIEW_LOCAL_INFORMATION)?;
    let mut buffer: *mut c_void = std::ptr::null_mut();
    // SAFETY: `policy` is open; `buffer` is a live out-pointer, freed by `LsaBuffer`.
    let status = unsafe {
        LsaQueryInformationPolicy(policy.0, PolicyAccountDomainInformation, &raw mut buffer)
    };
    lsa_check("LsaQueryInformationPolicy", status)?;
    let buffer = LsaBuffer(buffer);
    let unread = || MachineError::Other("this computer's account domain couldn't be read".into());
    if buffer.0.is_null() {
        return Err(unread());
    }
    // SAFETY: the call succeeded, so `buffer` holds a POLICY_ACCOUNT_DOMAIN_INFO, live until
    // `LsaBuffer` drops; its SID lies inside the same buffer.
    let sid = unsafe { sid_text((*buffer.0.cast::<POLICY_ACCOUNT_DOMAIN_INFO>()).DomainSid) }?;
    drop(buffer);
    drop(policy);
    sid.and_then(|sid| sid.domain()).ok_or_else(unread)
}

/// The SID of the local group `ClauseoSandboxUsers`, if it exists (`LookupAccountNameW`).
fn group_sid() -> Result<Option<Sid>, MachineError> {
    let name = wide(POOL_GROUP);
    let (mut sid_size, mut domain_size) = (0u32, 0u32);
    let mut kind = SID_NAME_USE::default();
    // SAFETY: the name is NUL-terminated and outlives the call; no buffers ask only for the
    // sizes, written to `sid_size` and `domain_size`.
    let sized = unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR(name.as_ptr()),
            None,
            &raw mut sid_size,
            None,
            &raw mut domain_size,
            &raw mut kind,
        )
    };
    match sized {
        Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {}
        Err(error) if error.code() == ERROR_NONE_MAPPED.to_hresult() => return Ok(None),
        Err(error) => return Err(os("LookupAccountNameW", &error)),
        Ok(()) => return Ok(None),
    }
    // u64 words, so the SID buffer is aligned.
    let mut sid = vec![0u64; usize::try_from(sid_size).unwrap_or(0).div_ceil(8).max(1)];
    let mut domain = vec![0u16; usize::try_from(domain_size).unwrap_or(0).max(1)];
    // SAFETY: as above; `sid` is writable for `sid_size` bytes and `domain` for
    // `domain_size` characters.
    unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR(name.as_ptr()),
            Some(PSID(sid.as_mut_ptr().cast())),
            &raw mut sid_size,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_size,
            &raw mut kind,
        )
    }
    .map_err(|error| os("LookupAccountNameW", &error))?;
    // A local group is an alias; an account of another kind by that name isn't the group.
    if kind != SidTypeAlias {
        return Ok(None);
    }
    // SAFETY: the call wrote a valid SID into `sid`, which is live.
    unsafe { sid_text(PSID(sid.as_mut_ptr().cast())) }
}

/// Opens the local LSA policy for `access`.
fn open_policy(access: i32) -> Result<Policy, MachineError> {
    let attributes = LSA_OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<LSA_OBJECT_ATTRIBUTES>()).unwrap_or(0),
        ..LSA_OBJECT_ATTRIBUTES::default()
    };
    let access = u32::try_from(access).unwrap_or(0);
    let mut handle = LSA_HANDLE::default();
    // SAFETY: no system name (the local one); `attributes` is zeroed with its length set, as
    // the call requires; `handle` is a live out-pointer, owned by `Policy` once opened.
    let status = unsafe { LsaOpenPolicy(None, &raw const attributes, access, &raw mut handle) };
    lsa_check("LsaOpenPolicy", status)?;
    Ok(Policy(handle))
}

/// `sid` in [`Sid`]'s form; `None` when its text isn't canonical.
///
/// # Safety
///
/// `sid` points to a valid SID for the call.
unsafe fn sid_text(sid: PSID) -> Result<Option<Sid>, MachineError> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` is valid, as the contract says; `text` is a live out-pointer, owned by
    // `Local` once written.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }
        .map_err(|error| os("ConvertSidToStringSidW", &error))?;
    let text = Local(text);
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let read = unsafe { text.0.to_string() };
    Ok(read.ok().and_then(|read| Sid::parse(&read).ok()))
}

/// An LSA call's status as a result: failures map to their Win32 code.
fn lsa_check(call: &'static str, status: NTSTATUS) -> Result<(), MachineError> {
    if status.0 >= 0 {
        return Ok(());
    }
    // SAFETY: LsaNtStatusToWinError only maps a number.
    let code = unsafe { LsaNtStatusToWinError(status) };
    Err(MachineError::Os { call, code })
}

/// A service manager or service handle, closed on drop.
struct Service(SC_HANDLE);

impl Drop for Service {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenSCManagerW or OpenServiceW and is closed once, here.
        if let Err(error) = unsafe { CloseServiceHandle(self.0) } {
            tracing::debug!(code = error.code().0, "CloseServiceHandle failed");
        }
    }
}

/// An LSA policy handle, closed on drop.
struct Policy(LSA_HANDLE);

impl Drop for Policy {
    fn drop(&mut self) {
        // SAFETY: the handle came from LsaOpenPolicy and is closed once, here.
        let status = unsafe { LsaClose(self.0) };
        if status.0 < 0 {
            tracing::debug!(status = status.0, "LsaClose failed");
        }
    }
}

/// A buffer the LSA allocated, freed on drop.
struct LsaBuffer(*mut c_void);

impl Drop for LsaBuffer {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: the buffer came from LsaEnumerateAccountsWithUserRight and is freed once.
        let status = unsafe { LsaFreeMemory(Some(self.0.cast_const())) };
        if status.0 < 0 {
            tracing::debug!(status = status.0, "LsaFreeMemory failed");
        }
    }
}

/// A string `LocalAlloc` allocated, freed on drop.
struct Local(PWSTR);

impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: the string came from ConvertSidToStringSidW (LocalAlloc) and is freed once.
        unsafe { LocalFree(Some(HLOCAL(self.0.0.cast()))) };
    }
}

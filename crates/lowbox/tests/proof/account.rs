//! The test account: made with `NetUserAdd`, given `SeInteractiveLogonRight` for the run (the
//! logon `CreateProcessWithLogonW` makes is interactive), then its right, the account and its
//! profile removed, each checked gone.
#![expect(
    unsafe_code,
    reason = "FFI to the NetUser, LookupAccountNameW, LSA, DeleteProfileW and registry calls"
)]

use std::ffi::c_void;

use clauseo_lowbox::Sid;
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, NTSTATUS, STATUS_OBJECT_NAME_NOT_FOUND,
};
use windows::Win32::NetworkManagement::NetManagement::{
    NERR_Success, NERR_UserNotFound, NetApiBufferFree, NetUserAdd, NetUserDel, NetUserGetInfo,
    UF_DONT_EXPIRE_PASSWD, UF_SCRIPT, USER_INFO_1, USER_PRIV_USER,
};
use windows::Win32::Security::Authentication::Identity::{
    LSA_HANDLE, LSA_OBJECT_ATTRIBUTES, LSA_UNICODE_STRING, LsaAddAccountRights, LsaClose,
    LsaEnumerateAccountRights, LsaFreeMemory, LsaNtStatusToWinError, LsaOpenPolicy,
    LsaRemoveAccountRights, POLICY_CREATE_ACCOUNT, POLICY_LOOKUP_NAMES,
};
use windows::Win32::Security::{LookupAccountNameW, PSID, SID_NAME_USE, SidTypeUser};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW,
};
use windows::core::{BOOL, PCWSTR, PWSTR};

use super::win::{Res, failed, last_error, sid_text, sleep_ms, wide};

windows::core::link!("userenv.dll" "system" fn DeleteProfileW(
    sid: PCWSTR,
    path: PCWSTR,
    computer: PCWSTR
) -> BOOL);

/// The right the run grants and takes back.
const LOGON_RIGHT: &str = "SeInteractiveLogonRight";
/// How many times `DeleteProfileW` is tried while the profile unloads.
const PROFILE_TRIES: u32 = 20;
/// The pause between those tries.
const PROFILE_PAUSE_MS: u32 = 3_000;

/// Makes the standard user `name` with `password` (never printed).
pub(crate) fn add(name: &str, password: &str) -> Res<()> {
    let mut user = wide(name);
    let mut secret = wide(password);
    let mut comment = wide("Clauseo lowbox proof row; the test deletes it");
    let info = USER_INFO_1 {
        usri1_name: PWSTR(user.as_mut_ptr()),
        usri1_password: PWSTR(secret.as_mut_ptr()),
        usri1_password_age: 0,
        usri1_priv: USER_PRIV_USER,
        usri1_home_dir: PWSTR::null(),
        usri1_comment: PWSTR(comment.as_mut_ptr()),
        usri1_flags: UF_SCRIPT | UF_DONT_EXPIRE_PASSWD,
        usri1_script_path: PWSTR::null(),
    };
    let mut bad = 0_u32;
    // SAFETY: `info` is a USER_INFO_1 whose strings are NUL-terminated buffers that outlive the
    // call; `bad` is a local.
    let rc = unsafe {
        NetUserAdd(
            PCWSTR::null(),
            1,
            (&raw const info).cast::<u8>(),
            Some(&raw mut bad),
        )
    };
    secret.fill(0);
    if rc != NERR_Success {
        return Err(format!("NetUserAdd failed with {rc} (parameter {bad})"));
    }
    Ok(())
}

/// Whether the local account `name` exists (`NetUserGetInfo` level 0).
pub(crate) fn exists(name: &str) -> Res<bool> {
    let user = wide(name);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    // SAFETY: a NUL-terminated name; `buffer` gets a NetApi buffer, freed below.
    let rc = unsafe { NetUserGetInfo(PCWSTR::null(), PCWSTR(user.as_ptr()), 0, &raw mut buffer) };
    if !buffer.is_null() {
        // SAFETY: the buffer came from NetUserGetInfo and is freed once, here.
        let _leak_on_failure: u32 =
            unsafe { NetApiBufferFree(Some(buffer.cast::<c_void>().cast_const())) };
    }
    if rc == NERR_Success {
        Ok(true)
    } else if rc == NERR_UserNotFound {
        Ok(false)
    } else {
        Err(format!("NetUserGetInfo failed with {rc}"))
    }
}

/// Deletes the account `name`, then checks `NetUserGetInfo` reports `NERR_UserNotFound`.
pub(crate) fn delete(name: &str) -> Res<String> {
    let user = wide(name);
    // SAFETY: a NUL-terminated name.
    let rc = unsafe { NetUserDel(PCWSTR::null(), PCWSTR(user.as_ptr())) };
    if rc != NERR_Success {
        return Err(format!("NetUserDel failed with {rc}"));
    }
    if exists(name)? {
        return Err("NetUserDel succeeded but NetUserGetInfo still finds the account".to_owned());
    }
    Ok("deleted; NetUserGetInfo reports NERR_UserNotFound".to_owned())
}

/// The SID text of the local user `name`.
pub(crate) fn lookup_sid(name: &str) -> Res<String> {
    let user = wide(name);
    let mut sid = vec![0_u64; 16];
    let mut sid_len = 128_u32;
    let mut domain = vec![0_u16; 256];
    let mut domain_len = 256_u32;
    let mut kind = SID_NAME_USE::default();
    // SAFETY: the buffers are writable for the lengths given and outlive the call.
    unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR(user.as_ptr()),
            Some(PSID(sid.as_mut_ptr().cast::<c_void>())),
            &raw mut sid_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_len,
            &raw mut kind,
        )
    }
    .map_err(|e| failed("LookupAccountNameW", &e))?;
    if kind != SidTypeUser {
        return Err(format!("LookupAccountNameW found a {kind:?}, not a user"));
    }
    sid_text(PSID(sid.as_mut_ptr().cast::<c_void>()))
}

/// The local LSA policy, closed on drop.
struct Policy(LSA_HANDLE);

impl Policy {
    /// Opens it for account rights.
    fn open() -> Res<Policy> {
        let attributes = LSA_OBJECT_ATTRIBUTES::default();
        let mut handle = LSA_HANDLE::default();
        let access = (POLICY_LOOKUP_NAMES | POLICY_CREATE_ACCOUNT).cast_unsigned();
        // SAFETY: zeroed attributes (as the docs ask) and a local handle, both outlive the call.
        let status = unsafe { LsaOpenPolicy(None, &raw const attributes, access, &raw mut handle) };
        lsa_ok("LsaOpenPolicy", status)?;
        Ok(Policy(handle))
    }
}

impl Drop for Policy {
    fn drop(&mut self) {
        // SAFETY: the handle came from LsaOpenPolicy and is closed once, here.
        let _close_failure_leaks: NTSTATUS = unsafe { LsaClose(self.0) };
    }
}

/// `Ok` for a success NTSTATUS from an LSA call, else the failure sentence.
fn lsa_ok(call: &str, status: NTSTATUS) -> Res<()> {
    if status.0 >= 0 {
        return Ok(());
    }
    // SAFETY: a pure conversion of the status.
    let win32 = unsafe { LsaNtStatusToWinError(status) };
    Err(format!(
        "{call} failed with NTSTATUS {:#010x} (Win32 {win32})",
        status.0.cast_unsigned()
    ))
}

/// The logon right as an LSA string over `units` (no NUL).
fn right(units: &mut [u16]) -> LSA_UNICODE_STRING {
    let bytes = u16::try_from(units.len() * 2).unwrap_or(u16::MAX);
    LSA_UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(units.as_mut_ptr()),
    }
}

/// The binary SID of `sid_text`.
fn sid_bytes(sid_text: &str) -> Res<Vec<u8>> {
    Sid::parse(sid_text)
        .map(|s| s.to_bytes())
        .map_err(|e| format!("the account's SID text: {e}"))
}

/// Grants `SeInteractiveLogonRight` to the account `sid`.
pub(crate) fn grant_logon(sid: &str) -> Res<()> {
    let policy = Policy::open()?;
    let mut sid = sid_bytes(sid)?;
    let mut units: Vec<u16> = LOGON_RIGHT.encode_utf16().collect();
    let rights = [right(&mut units)];
    // SAFETY: an open policy, a binary SID and one LSA string, all outliving the call.
    let status =
        unsafe { LsaAddAccountRights(policy.0, PSID(sid.as_mut_ptr().cast::<c_void>()), &rights) };
    lsa_ok("LsaAddAccountRights", status)
}

/// The rights the account `sid` holds itself (not through its groups).
fn rights_of(policy: &Policy, sid: &mut [u8]) -> Res<Vec<String>> {
    let mut list: *mut LSA_UNICODE_STRING = std::ptr::null_mut();
    let mut count = 0_u32;
    // SAFETY: an open policy and a binary SID; `list` gets an LSA buffer, freed below.
    let status = unsafe {
        LsaEnumerateAccountRights(
            policy.0,
            PSID(sid.as_mut_ptr().cast::<c_void>()),
            &raw mut list,
            &raw mut count,
        )
    };
    if status == STATUS_OBJECT_NAME_NOT_FOUND {
        return Ok(Vec::new());
    }
    lsa_ok("LsaEnumerateAccountRights", status)?;
    let count = usize::try_from(count).unwrap_or(0);
    let names = (0..count)
        .map(|i| {
            // SAFETY: the call succeeded, so `list` holds `count` strings, each `Length` bytes.
            unsafe {
                let one = &*list.add(i);
                if one.Buffer.is_null() {
                    return String::new();
                }
                let units = std::slice::from_raw_parts(one.Buffer.0, usize::from(one.Length / 2));
                String::from_utf16_lossy(units)
            }
        })
        .collect();
    // SAFETY: the buffer came from LsaEnumerateAccountRights and is freed once, here.
    let _leak_on_failure: NTSTATUS =
        unsafe { LsaFreeMemory(Some(list.cast::<c_void>().cast_const())) };
    Ok(names)
}

/// Takes `SeInteractiveLogonRight` back from the account `sid`, then checks
/// `LsaEnumerateAccountRights` no longer lists it.
pub(crate) fn remove_logon(sid: &str) -> Res<String> {
    let policy = Policy::open()?;
    let mut sid = sid_bytes(sid)?;
    let mut units: Vec<u16> = LOGON_RIGHT.encode_utf16().collect();
    let rights = [right(&mut units)];
    // SAFETY: as in `grant_logon`.
    let status = unsafe {
        LsaRemoveAccountRights(
            policy.0,
            PSID(sid.as_mut_ptr().cast::<c_void>()),
            false,
            Some(&rights),
        )
    };
    if status != STATUS_OBJECT_NAME_NOT_FOUND {
        lsa_ok("LsaRemoveAccountRights", status)?;
    }
    let left = rights_of(&policy, &mut sid)?;
    if left.iter().any(|r| r == LOGON_RIGHT) {
        return Err("LsaEnumerateAccountRights still lists SeInteractiveLogonRight".to_owned());
    }
    Ok(format!(
        "removed; LsaEnumerateAccountRights lists {} right(s), not {LOGON_RIGHT}",
        left.len()
    ))
}

/// Whether `HKLM\...\ProfileList\<sid>` exists.
fn profile_key_exists(sid: &str) -> Res<bool> {
    let path = wide(format!(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\{sid}"
    ));
    let mut key = HKEY::default();
    // SAFETY: a NUL-terminated path; `key` gets a key handle, closed below.
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(path.as_ptr()),
            Some(0),
            KEY_READ,
            &raw mut key,
        )
    };
    if rc == ERROR_SUCCESS {
        // SAFETY: the key was just opened and is closed once, here.
        let _leak_on_failure = unsafe { RegCloseKey(key) }.0;
        return Ok(true);
    }
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    Err(format!("RegOpenKeyExW(ProfileList) failed with {}", rc.0))
}

/// Deletes the profile of the account `sid` (retrying while it unloads), then checks its
/// `ProfileList` key is gone.
pub(crate) fn delete_profile(sid: &str) -> Res<String> {
    let text = wide(sid);
    let mut last = 0_u32;
    for attempt in 1..=PROFILE_TRIES {
        // SAFETY: a NUL-terminated SID string; no path and no computer name.
        let ok = unsafe { DeleteProfileW(PCWSTR(text.as_ptr()), PCWSTR::null(), PCWSTR::null()) };
        if ok.as_bool() {
            if profile_key_exists(sid)? {
                return Err("DeleteProfileW succeeded but the ProfileList key remains".to_owned());
            }
            return Ok(format!(
                "deleted on try {attempt}; the ProfileList key is gone"
            ));
        }
        last = last_error();
        if !profile_key_exists(sid)? {
            return Ok(format!(
                "DeleteProfileW error {last} on try {attempt}, and the ProfileList key is gone"
            ));
        }
        sleep_ms(PROFILE_PAUSE_MS);
    }
    Err(format!(
        "DeleteProfileW still failing after {PROFILE_TRIES} tries (last error {last}); the \
         ProfileList key remains"
    ))
}

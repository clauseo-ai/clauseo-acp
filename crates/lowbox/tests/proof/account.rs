//! The test account: made with `NetUserAdd`, given `SeInteractiveLogonRight` for the run (the
//! logon `CreateProcessWithLogonW` makes is interactive), then, once its processes are gone,
//! its rights, the account and its profile removed, each checked gone.
#![expect(
    unsafe_code,
    reason = "FFI to the NetUser, LookupAccountNameW, LSA, DeleteProfileW, registry and WTS calls"
)]

use std::ffi::c_void;
use std::path::{Path, PathBuf};

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
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW, RegOpenKeyExW,
};
use windows::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_PROCESS_INFOW, WTSEnumerateProcessesW, WTSFreeMemory,
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
/// How many times the account's processes are looked for (30 s at [`GONE_PAUSE_MS`]).
const GONE_TRIES: u32 = 60;
/// The pause between those looks.
const GONE_PAUSE_MS: u32 = 500;
/// The `ProfileList` key, below `HKLM`.
const PROFILE_LIST: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";

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

/// The rights the account `sid` holds itself (not through its groups); `None` when LSA has no
/// account record for it (`STATUS_OBJECT_NAME_NOT_FOUND`).
fn rights_of(policy: &Policy, sid: &mut [u8]) -> Res<Option<Vec<String>>> {
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
        return Ok(None);
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
    Ok(Some(names))
}

/// Takes every right back from the account `sid` (`AllRights`, which also deletes its LSA
/// account record), then checks `LsaEnumerateAccountRights` finds no record or an empty list.
pub(crate) fn remove_logon(sid: &str) -> Res<String> {
    let policy = Policy::open()?;
    let mut sid = sid_bytes(sid)?;
    // SAFETY: an open policy and a binary SID that outlives the call; no list with AllRights.
    let status = unsafe {
        LsaRemoveAccountRights(
            policy.0,
            PSID(sid.as_mut_ptr().cast::<c_void>()),
            true,
            None,
        )
    };
    if status != STATUS_OBJECT_NAME_NOT_FOUND {
        lsa_ok("LsaRemoveAccountRights(AllRights)", status)?;
    }
    match rights_of(&policy, &mut sid)? {
        None => Ok("removed all rights; LsaEnumerateAccountRights reports \
                    STATUS_OBJECT_NAME_NOT_FOUND"
            .to_owned()),
        Some(left) if left.is_empty() => {
            Ok("removed all rights; LsaEnumerateAccountRights lists none".to_owned())
        }
        Some(left) => Err(format!(
            "LsaEnumerateAccountRights still lists {}",
            left.join(", ")
        )),
    }
}

/// The ids of the running processes whose token user is `sid` (`WTSEnumerateProcessesW`, which
/// gives each process's user without opening it).
fn processes_of(sid: &str) -> Res<Vec<u32>> {
    let mut list: *mut WTS_PROCESS_INFOW = std::ptr::null_mut();
    let mut count = 0_u32;
    // SAFETY: the local server, version 1; `list` gets a WTS buffer, freed below.
    unsafe {
        WTSEnumerateProcessesW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            0,
            1,
            &raw mut list,
            &raw mut count,
        )
    }
    .map_err(|e| failed("WTSEnumerateProcessesW", &e))?;
    let mut pids = Vec::new();
    for i in 0..usize::try_from(count).unwrap_or(0) {
        // SAFETY: the call succeeded, so `list` holds `count` entries, alive until the free.
        let one = unsafe { &*list.add(i) };
        if one.pUserSid.0.is_null() {
            continue;
        }
        if sid_text(one.pUserSid).is_ok_and(|user| user.eq_ignore_ascii_case(sid)) {
            pids.push(one.ProcessId);
        }
    }
    // SAFETY: the buffer came from WTSEnumerateProcessesW and is freed once, here.
    unsafe { WTSFreeMemory(list.cast::<c_void>()) };
    Ok(pids)
}

/// Waits (at most 30 s) until no process of the account `sid` runs.
pub(crate) fn wait_processes_gone(sid: &str) -> Res<String> {
    let mut left = Vec::new();
    for look in 1..=GONE_TRIES {
        left = processes_of(sid)?;
        if left.is_empty() {
            return Ok(format!("none left (look {look})"));
        }
        sleep_ms(GONE_PAUSE_MS);
    }
    Err(format!(
        "{} process(es) of the account still run after 30 s: pids {left:?}",
        left.len()
    ))
}

/// Whether `HKLM\...\ProfileList\<sid>` exists.
fn profile_key_exists(sid: &str) -> Res<bool> {
    let path = wide(format!(r"{PROFILE_LIST}\{sid}"));
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

/// The account's profile folder from its `ProfileImagePath`, or `None` without a key.
fn profile_folder(sid: &str) -> Res<Option<PathBuf>> {
    let key = wide(format!(r"{PROFILE_LIST}\{sid}"));
    let value = wide("ProfileImagePath");
    let mut data = vec![0_u16; 1024];
    let mut bytes = u32::try_from(data.len() * 2).unwrap_or(u32::MAX);
    // SAFETY: NUL-terminated names and a writable buffer of `bytes` bytes; REG_EXPAND_SZ comes
    // back expanded under RRF_RT_REG_SZ.
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(data.as_mut_ptr().cast::<c_void>()),
            Some(&raw mut bytes),
        )
    };
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if rc != ERROR_SUCCESS {
        return Err(format!(
            "RegGetValueW(ProfileImagePath) failed with {}",
            rc.0
        ));
    }
    let len = data.iter().position(|&u| u == 0).unwrap_or(data.len());
    Ok(Some(PathBuf::from(String::from_utf16_lossy(&data[..len]))))
}

/// Deletes the profile of the account `sid` (retrying while it unloads), then checks both its
/// `ProfileList` key and its folder are gone. The folder is read from `ProfileImagePath`
/// first, else `fallback` (`%SystemDrive%\Users\<name>`).
pub(crate) fn delete_profile(sid: &str, fallback: &Path) -> Res<String> {
    let (folder, from) = match profile_folder(sid)? {
        Some(folder) => (folder, "ProfileImagePath"),
        None => (fallback.to_owned(), "the fallback"),
    };
    let text = wide(sid);
    let mut outcome = String::new();
    for attempt in 1..=PROFILE_TRIES {
        // SAFETY: a NUL-terminated SID string; no path and no computer name.
        let ok = unsafe { DeleteProfileW(PCWSTR(text.as_ptr()), PCWSTR::null(), PCWSTR::null()) };
        if ok.as_bool() {
            outcome = format!("deleted on try {attempt}");
            break;
        }
        outcome = format!("DeleteProfileW error {} on try {attempt}", last_error());
        if !profile_key_exists(sid)? {
            break;
        }
        if attempt < PROFILE_TRIES {
            sleep_ms(PROFILE_PAUSE_MS);
        }
    }
    let key = profile_key_exists(sid)?;
    let folder_left = folder
        .try_exists()
        .map_err(|e| format!("{outcome}; checking the profile folder: {e}"))?;
    match (key, folder_left) {
        (false, false) => Ok(format!(
            "{outcome}; the ProfileList key and the profile folder (from {from}) are gone"
        )),
        (true, _) => Err(format!(
            "{outcome}; the ProfileList key remains (folder remains: {folder_left})"
        )),
        (false, true) => Err(format!(
            "{outcome}; the ProfileList key is gone but the profile folder {} remains",
            folder.display()
        )),
    }
}

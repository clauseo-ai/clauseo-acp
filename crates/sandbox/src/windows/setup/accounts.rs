//! The account calls of `WindowsMachine`: the pool users and their group on this machine
//! (`NetUserAdd`, `NetUserSetInfo`, `NetUserDel`, `NetLocalGroupAdd`,
//! `NetLocalGroupAddMembers`, `NetLocalGroupGetMembers`, `NetLocalGroupDel`), their SIDs
//! (`LookupAccountNameW`, `LookupAccountSidW`), and the sign-in screen's hidden list
//! (`SpecialAccounts\UserList`, as Codex's `hide_users.rs` uses it; no code taken).
//!
//! Every lookup is of this machine's own accounts: a name is looked up as
//! `<computer>\<name>`, and a SID only counts when its domain is this computer, so a domain
//! account with a pool user's name is never taken for one. A password is passed in a buffer
//! that is wiped when the call returns, and never appears in an error.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 account, SID and registry calls"
)]

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_MEMBER_IN_ALIAS, ERROR_NONE_MAPPED,
    HLOCAL, LocalFree,
};
use windows::Win32::NetworkManagement::NetManagement::{
    LOCALGROUP_INFO_1, LOCALGROUP_MEMBERS_INFO_0, MAX_PREFERRED_LENGTH, NERR_Success,
    NERR_UserNotFound, NetApiBufferFree, NetLocalGroupAdd, NetLocalGroupAddMembers,
    NetLocalGroupDel, NetLocalGroupGetMembers, NetUserAdd, NetUserDel, NetUserSetInfo,
    UF_DONT_EXPIRE_PASSWD, UF_PASSWD_CANT_CHANGE, UF_SCRIPT, USER_INFO_1, USER_INFO_1003,
    USER_PRIV_USER,
};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW};
use windows::Win32::Security::{
    LookupAccountNameW, LookupAccountSidW, PSID, SID_NAME_USE, SidTypeAlias, SidTypeUser,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_OPTION_NON_VOLATILE,
    RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW,
};
use windows::Win32::System::SystemInformation::{ComputerNameNetBIOS, GetComputerNameExW};
use windows::core::{HRESULT, HSTRING, PCWSTR, PWSTR};
use zeroize::Zeroizing;

use super::MachineError;
use super::lock::os;
use super::password::is_strong;
use super::record_values::RegValue;
use super::record_write::{Key, check, set_value};
use crate::windows::sids::Sid;

/// The key whose `DWORD` value 0 under an account's name hides it from the sign-in screen.
const HIDDEN_USERS: &str =
    "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon\\SpecialAccounts\\UserList";

/// The built-in `Users` group's SID; its name depends on Windows' language.
const USERS: &str = "S-1-5-32-545";

/// The longest local account or group name Windows takes.
const NAME_MAX: usize = 20;

/// Which kind of account a lookup wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// A user account (`SidTypeUser`).
    User,
    /// A local group (`SidTypeAlias`).
    Group,
}

impl Kind {
    fn matches(self, found: SID_NAME_USE) -> bool {
        match self {
            Kind::User => found == SidTypeUser,
            Kind::Group => found == SidTypeAlias,
        }
    }
}

/// The SID of this machine's account `name` of kind `kind`; `None` when there is none.
pub(super) fn sid_of(name: &str, kind: Kind) -> Result<Option<Sid>, MachineError> {
    checked_name(name)?;
    let computer = computer_name()?;
    let qualified = HSTRING::from(format!("{computer}\\{name}"));
    let (mut sid_len, mut domain_len) = (0u32, 0u32);
    let mut used = SID_NAME_USE::default();
    // SAFETY: no buffers ask only for the sizes, written to the live out-pointers.
    let sized = unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            &qualified,
            None,
            &raw mut sid_len,
            None,
            &raw mut domain_len,
            &raw mut used,
        )
    };
    match sized {
        Err(error) if error.code() == HRESULT::from_win32(ERROR_NONE_MAPPED.0) => return Ok(None),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_INSUFFICIENT_BUFFER.0) => {}
        Err(error) => return Err(os("LookupAccountNameW", &error)),
        Ok(()) => {
            return Err(MachineError::Other(
                "LookupAccountNameW gave no size".into(),
            ));
        }
    }
    // u64 words, so the SID is aligned.
    let mut sid = vec![0u64; usize::try_from(sid_len).unwrap_or(0).div_ceil(8).max(1)];
    let mut domain = vec![0u16; usize::try_from(domain_len).unwrap_or(0).max(1)];
    // SAFETY: `sid` holds at least `sid_len` bytes and `domain` `domain_len` units, both live and
    // writable for the call; the other pointers are live out-pointers.
    unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            &qualified,
            Some(PSID(sid.as_mut_ptr().cast())),
            &raw mut sid_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_len,
            &raw mut used,
        )
    }
    .map_err(|error| os("LookupAccountNameW", &error))?;
    if !kind.matches(used) || !same_name(&text_of(&domain), &computer) {
        return Ok(None);
    }
    sid_text(PSID(sid.as_mut_ptr().cast())).map(Some)
}

/// The name of this machine's account `sid` of kind `kind`; `None` when no local account of
/// that kind has it.
pub(super) fn name_of(sid: &Sid, kind: Kind) -> Result<Option<String>, MachineError> {
    let Some((name, domain, used)) = lookup_sid(sid)? else {
        return Ok(None);
    };
    Ok((kind.matches(used) && same_name(&domain, &computer_name()?)).then_some(name))
}

/// Makes the local account `name` with `password`: `NetUserAdd` level 1, a standard user,
/// `UF_SCRIPT | UF_DONT_EXPIRE_PASSWD | UF_PASSWD_CANT_CHANGE`. An account by that name is
/// `NERR_UserExists` (2224), never adopted (D-B5). Its SID; when the SID can't be read the
/// account is deleted again, so a failure leaves none behind.
pub(super) fn create_user(name: &str, password: &str) -> Result<Sid, MachineError> {
    checked_name(name)?;
    checked_password(password)?;
    let mut name_units = wide(name);
    let mut secret = secret_units(password);
    let info = USER_INFO_1 {
        usri1_name: PWSTR(name_units.as_mut_ptr()),
        usri1_password: PWSTR(secret.as_mut_ptr()),
        usri1_password_age: 0,
        usri1_priv: USER_PRIV_USER,
        usri1_home_dir: PWSTR::null(),
        usri1_comment: PWSTR::null(),
        usri1_flags: UF_SCRIPT | UF_DONT_EXPIRE_PASSWD | UF_PASSWD_CANT_CHANGE,
        usri1_script_path: PWSTR::null(),
    };
    // SAFETY: `info` and the buffers it points to live for the call, each NUL-terminated.
    let status = unsafe { NetUserAdd(PCWSTR::null(), 1, (&raw const info).cast(), None) };
    drop(secret);
    net("NetUserAdd", status)?;
    match sid_of(name, Kind::User) {
        Ok(Some(sid)) => Ok(sid),
        found => {
            // The account exists but its SID can't be read: undo it. Its own failure would
            // only hide the first one.
            drop(delete_user(name));
            Err(found
                .err()
                .unwrap_or_else(|| MachineError::Other("the new account has no SID".to_owned())))
        }
    }
}

/// Sets the local account `name`'s password: `NetUserSetInfo` level 1003.
pub(super) fn set_password(name: &str, password: &str) -> Result<(), MachineError> {
    checked_name(name)?;
    checked_password(password)?;
    let mut secret = secret_units(password);
    let info = USER_INFO_1003 {
        usri1003_password: PWSTR(secret.as_mut_ptr()),
    };
    let account = HSTRING::from(name);
    // SAFETY: `info` and the password buffer live for the call; the name is NUL-terminated.
    let status = unsafe {
        NetUserSetInfo(
            PCWSTR::null(),
            &account,
            1003,
            (&raw const info).cast(),
            None,
        )
    };
    drop(secret);
    net("NetUserSetInfo", status)
}

/// Deletes the local account `name` (`NetUserDel`) and its sign-in screen value. An account
/// or value already gone is `Ok`.
pub(super) fn delete_user(name: &str) -> Result<(), MachineError> {
    checked_name(name)?;
    let account = HSTRING::from(name);
    // SAFETY: the name is a NUL-terminated HSTRING that lives for the call.
    let status = unsafe { NetUserDel(PCWSTR::null(), &account) };
    if status != NERR_UserNotFound {
        net("NetUserDel", status)?;
    }
    unhide(name)
}

/// Makes the local group `name` (`NetLocalGroupAdd` level 1); its SID. A group by that name is
/// `NERR_GroupExists` (2223) or `ERROR_ALIAS_EXISTS` (1379).
pub(super) fn create_group(name: &str, comment: &str) -> Result<Sid, MachineError> {
    checked_name(name)?;
    let mut name_units = wide(name);
    let mut comment_units = wide(comment);
    let info = LOCALGROUP_INFO_1 {
        lgrpi1_name: PWSTR(name_units.as_mut_ptr()),
        lgrpi1_comment: PWSTR(comment_units.as_mut_ptr()),
    };
    // SAFETY: `info` and the buffers it points to live for the call, each NUL-terminated.
    let status = unsafe { NetLocalGroupAdd(PCWSTR::null(), 1, (&raw const info).cast(), None) };
    net("NetLocalGroupAdd", status)?;
    sid_of(name, Kind::Group)?
        .ok_or_else(|| MachineError::Other("the new group has no SID".to_owned()))
}

/// Deletes the local group `name` (`NetLocalGroupDel`).
pub(super) fn delete_group(name: &str) -> Result<(), MachineError> {
    checked_name(name)?;
    let group = HSTRING::from(name);
    // SAFETY: the name is a NUL-terminated HSTRING that lives for the call.
    let status = unsafe { NetLocalGroupDel(PCWSTR::null(), &group) };
    net("NetLocalGroupDel", status)
}

/// How many members the local group `name` has (`NetLocalGroupGetMembers` level 0).
pub(super) fn group_members(name: &str) -> Result<usize, MachineError> {
    checked_name(name)?;
    let group = HSTRING::from(name);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    let (mut read, mut total) = (0u32, 0u32);
    // SAFETY: the name is NUL-terminated; the out-pointers are live. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe {
        NetLocalGroupGetMembers(
            PCWSTR::null(),
            &group,
            0,
            &raw mut buffer,
            MAX_PREFERRED_LENGTH,
            &raw mut read,
            &raw mut total,
            None,
        )
    };
    let _freed = NetBuffer(buffer);
    net("NetLocalGroupGetMembers", status)?;
    usize::try_from(total).map_err(|error| MachineError::Other(error.to_string()))
}

/// Puts the account `member` in the local group `group` (`NetLocalGroupAddMembers` level 0,
/// by SID). A membership already there (`ERROR_MEMBER_IN_ALIAS`) is `Ok`.
pub(super) fn add_member(group: &str, member: &Sid) -> Result<(), MachineError> {
    checked_name(group)?;
    let sid = LocalSid::parse(member)?;
    let info = LOCALGROUP_MEMBERS_INFO_0 { lgrmi0_sid: sid.0 };
    let group = HSTRING::from(group);
    // SAFETY: `info` holds one valid SID that lives for the call; the name is NUL-terminated.
    let status =
        unsafe { NetLocalGroupAddMembers(PCWSTR::null(), &group, 0, (&raw const info).cast(), 1) };
    if status == ERROR_MEMBER_IN_ALIAS.0 {
        return Ok(());
    }
    net("NetLocalGroupAddMembers", status)
}

/// The built-in `Users` group's name in this Windows' language.
pub(super) fn users_group() -> Result<String, MachineError> {
    let users = Sid::parse(USERS).map_err(|error| MachineError::Other(error.to_string()))?;
    lookup_sid(&users)?
        .map(|(name, _, _)| name)
        .ok_or_else(|| MachineError::Other("the Users group has no name".to_owned()))
}

/// Hides `name` from the sign-in screen: `DWORD` 0 named `name` under [`HIDDEN_USERS`] (made
/// when missing, in the 64-bit view, keeping its inherited DACL).
pub(super) fn hide(name: &str) -> Result<(), MachineError> {
    checked_name(name)?;
    let subkey = HSTRING::from(HIDDEN_USERS);
    let mut opened = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `opened` is a live out-pointer, closed by `Key`.
    let made = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            None,
            &raw mut opened,
            None,
        )
    };
    check("RegCreateKeyExW", made)?;
    set_value(&Key(opened), name, &RegValue::Dword(0))
}

/// Removes `name`'s sign-in screen value. A missing key or value is `Ok`.
fn unhide(name: &str) -> Result<(), MachineError> {
    let subkey = HSTRING::from(HIDDEN_USERS);
    let mut opened = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `opened` is a live out-pointer, closed by `Key`.
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            &raw mut opened,
        )
    };
    if open == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check("RegOpenKeyExW", open)?;
    let opened = Key(opened);
    let value = HSTRING::from(name);
    // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated.
    let deleted = unsafe { RegDeleteValueW(opened.0, &value) };
    if deleted == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check("RegDeleteValueW", deleted)
}

/// Whether `name`'s sign-in screen value is there, for the tests.
#[cfg(test)]
pub(crate) fn hidden(name: &str) -> Result<bool, MachineError> {
    let key = HSTRING::from(HIDDEN_USERS);
    let value = HSTRING::from(name);
    crate::windows::record::read_dword(PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()))
        .map(|found| found == Some(0))
        .map_err(|error| MachineError::Other(error.to_string()))
}

/// The account `sid` names, as (name, domain, use); `None` when it names none.
fn lookup_sid(sid: &Sid) -> Result<Option<(String, String, SID_NAME_USE)>, MachineError> {
    let parsed = LocalSid::parse(sid)?;
    let (mut name_len, mut domain_len) = (0u32, 0u32);
    let mut used = SID_NAME_USE::default();
    // SAFETY: the SID is valid for the call; no buffers ask only for the sizes.
    let sized = unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            parsed.0,
            None,
            &raw mut name_len,
            None,
            &raw mut domain_len,
            &raw mut used,
        )
    };
    match sized {
        Err(error) if error.code() == HRESULT::from_win32(ERROR_NONE_MAPPED.0) => return Ok(None),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_INSUFFICIENT_BUFFER.0) => {}
        Err(error) => return Err(os("LookupAccountSidW", &error)),
        Ok(()) => return Err(MachineError::Other("LookupAccountSidW gave no size".into())),
    }
    let mut name = vec![0u16; usize::try_from(name_len).unwrap_or(0).max(1)];
    let mut domain = vec![0u16; usize::try_from(domain_len).unwrap_or(0).max(1)];
    // SAFETY: `name` holds `name_len` units and `domain` `domain_len`, live and writable for the
    // call; the SID is valid.
    unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            parsed.0,
            Some(PWSTR(name.as_mut_ptr())),
            &raw mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_len,
            &raw mut used,
        )
    }
    .map_err(|error| os("LookupAccountSidW", &error))?;
    Ok(Some((text_of(&name), text_of(&domain), used)))
}

/// This computer's `NetBIOS` name, the domain of its local accounts.
fn computer_name() -> Result<String, MachineError> {
    let mut size = 0u32;
    // SAFETY: no buffer asks only for the size, written to `size`; the expected failure is
    // ERROR_MORE_DATA.
    drop(unsafe { GetComputerNameExW(ComputerNameNetBIOS, None, &raw mut size) });
    let mut buffer = vec![0u16; usize::try_from(size).unwrap_or(0).max(16)];
    let mut len = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: `buffer` is live and writable for `len` units.
    unsafe {
        GetComputerNameExW(
            ComputerNameNetBIOS,
            Some(PWSTR(buffer.as_mut_ptr())),
            &raw mut len,
        )
    }
    .map_err(|error| os("GetComputerNameExW", &error))?;
    Ok(text_of(&buffer))
}

/// Whether two account or computer names are the same, as Windows compares them (without case).
fn same_name(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

/// `name`, if Windows can take it as a local account name that only names itself: 1 to 20
/// characters, none of which qualifies it with a domain or computer.
fn checked_name(name: &str) -> Result<(), MachineError> {
    let fits = (1..=NAME_MAX).contains(&name.chars().count());
    if fits && !name.contains(['\\', '/', '@', '\0']) {
        Ok(())
    } else {
        Err(MachineError::Other(format!(
            "{name:?} is not a local account name"
        )))
    }
}

/// Refuses any password the generator could not have given (an empty one included, which a
/// failed random source leaves), without naming it.
fn checked_password(password: &str) -> Result<(), MachineError> {
    if is_strong(password) {
        Ok(())
    } else {
        Err(MachineError::Other(
            "the account's random password could not be made".to_owned(),
        ))
    }
}

/// `text`'s UTF-16 units and a NUL.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// `password`'s UTF-16 units and a NUL, in a buffer sized up front (so no copy is left behind
/// when it grows) and wiped on drop.
fn secret_units(password: &str) -> Zeroizing<Vec<u16>> {
    let mut units = Zeroizing::new(Vec::with_capacity(password.len() + 1));
    units.extend(password.encode_utf16());
    units.push(0);
    units
}

/// The text in `units` up to the first NUL.
fn text_of(units: &[u16]) -> String {
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    String::from_utf16_lossy(units.get(..end).unwrap_or_default())
}

/// A `NET_API_STATUS` as `Ok`, or [`MachineError::Os`] naming `call`.
fn net(call: &'static str, status: u32) -> Result<(), MachineError> {
    if status == NERR_Success {
        Ok(())
    } else {
        Err(MachineError::Os { call, code: status })
    }
}

/// `sid`'s text, from a valid SID.
fn sid_text(sid: PSID) -> Result<Sid, MachineError> {
    let mut text = PWSTR::null();
    // SAFETY: the caller's SID is valid; `text` is a live out-pointer, freed below.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }
        .map_err(|error| os("ConvertSidToStringSidW", &error))?;
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let read = unsafe { text.to_string() };
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    let read = read.map_err(|error| MachineError::Other(error.to_string()))?;
    Sid::parse(&read).map_err(|error| MachineError::Other(error.to_string()))
}

/// A buffer the `Net*` calls allocated; freed with `NetApiBufferFree` on drop.
pub(super) struct NetBuffer(pub(super) *mut u8);

impl Drop for NetBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the buffer came from a Net* call and is freed once, here. A failed free
            // leaves nothing to do.
            unsafe { NetApiBufferFree(Some(self.0.cast_const().cast())) };
        }
    }
}

/// A SID `ConvertStringSidToSidW` allocated; freed with `LocalFree` on drop.
pub(super) struct LocalSid(pub(super) PSID);

impl LocalSid {
    /// The SID `sid` spells, allocated for the Win32 calls.
    pub(super) fn parse(sid: &Sid) -> Result<LocalSid, MachineError> {
        let text = HSTRING::from(sid.as_str());
        let mut parsed = PSID::default();
        // SAFETY: the text is NUL-terminated and lives for the call; `parsed` is a live
        // out-pointer, freed on drop.
        unsafe { ConvertStringSidToSidW(&text, &raw mut parsed) }
            .map_err(|error| os("ConvertStringSidToSidW", &error))?;
        Ok(LocalSid(parsed))
    }
}

impl Drop for LocalSid {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSidToSidW with LocalAlloc, freed once, here.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_local_and_short() {
        for good in [
            "ClauseoSb01",
            "ClauseoSandboxUsers",
            "a",
            "ClauseoSbTest1234567",
        ] {
            assert!(checked_name(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "ClauseoSbTest12345678",
            "PC\\ClauseoSb01",
            "a@b",
            "a/b",
            "a\0",
        ] {
            assert!(checked_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_weak_password_is_refused_without_being_named() {
        let error = checked_password("hunter2").unwrap_err();
        assert!(!error.to_string().contains("hunter2"), "{error}");
        assert!(checked_password("").is_err());
        let strong = super::super::password::generate().unwrap();
        assert!(checked_password(&strong).is_ok());
    }
}

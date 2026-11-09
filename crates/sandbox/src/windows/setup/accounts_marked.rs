//! Telling Clauseo's own accounts and group from ones that only share their names (`intent.rs`):
//! each pool account is made with [`USER_COMMENT`] (`accounts::create_user`) and the group with
//! [`GROUP_COMMENT`]; an account in the pool's group counts as ours too. Read with
//! `NetUserGetInfo` (level 10), `NetUserGetLocalGroups` (level 0) and `NetLocalGroupGetInfo`
//! (level 1), on this machine only.
#![expect(unsafe_code, reason = "FFI to the Win32 Net* calls")]

use windows::Win32::Foundation::ERROR_NO_SUCH_ALIAS;
use windows::Win32::NetworkManagement::NetManagement::{
    LOCALGROUP_INFO_1, LOCALGROUP_USERS_INFO_0, MAX_PREFERRED_LENGTH, NERR_GroupNotFound,
    NERR_UserNotFound, NetLocalGroupGetInfo, NetUserGetInfo, NetUserGetLocalGroups, USER_INFO_10,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::MachineError;
use super::accounts::{self, Kind, NetBuffer, net};
use super::windows_machine::Names;
use crate::windows::sids::Sid;

/// The comment every pool account is made with, which Computer Management shows; [`marked_user`]
/// reads it back.
pub(crate) const USER_COMMENT: &str = "Clauseo's protection runs a matter's agent as this user.";

/// The comment the pool group carries in Computer Management; [`marked_group`] reads it back.
pub(super) const GROUP_COMMENT: &str =
    "Clauseo's protection runs each matter's agent as one of these users.";

/// The SID of the engine's account `name` (as `names` makes it real) when it is there and ours:
/// it carries [`USER_COMMENT`], or is a member of the pool's group. `None` otherwise.
pub(super) fn marked_user(names: &Names, name: &str) -> Result<Option<Sid>, MachineError> {
    let real = names.real(name);
    let Some(sid) = accounts::sid_of(&real, Kind::User)? else {
        return Ok(None);
    };
    let ours = user_comment(&real)?.as_deref() == Some(USER_COMMENT)
        || local_groups(&real)?
            .iter()
            .any(|group| group.to_lowercase() == names.group.to_lowercase());
    Ok(ours.then_some(sid))
}

/// The SID of the pool's group (as `names` names it) when it is there and carries
/// [`GROUP_COMMENT`].
pub(super) fn marked_group(names: &Names) -> Result<Option<Sid>, MachineError> {
    let Some(sid) = accounts::sid_of(&names.group, Kind::Group)? else {
        return Ok(None);
    };
    Ok((group_comment(&names.group)?.as_deref() == Some(GROUP_COMMENT)).then_some(sid))
}

/// The local account `name`'s comment; `None` when there is no such account.
fn user_comment(name: &str) -> Result<Option<String>, MachineError> {
    let account = HSTRING::from(name);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    // SAFETY: the name is NUL-terminated; the out-pointer is live. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe { NetUserGetInfo(PCWSTR::null(), &account, 10, &raw mut buffer) };
    let buffer = NetBuffer(buffer);
    if status == NERR_UserNotFound {
        return Ok(None);
    }
    net("NetUserGetInfo", status)?;
    if buffer.0.is_null() {
        return Ok(None);
    }
    // SAFETY: level 10 fills one USER_INFO_10, which lives until `buffer` is dropped; its
    // strings live there too.
    let info = unsafe { buffer.0.cast::<USER_INFO_10>().read_unaligned() };
    text(info.usri10_comment).map(Some)
}

/// The local groups the account `name` is directly in; none when there is no such account.
fn local_groups(name: &str) -> Result<Vec<String>, MachineError> {
    let account = HSTRING::from(name);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    let (mut read, mut total) = (0u32, 0u32);
    // SAFETY: the name is NUL-terminated; the out-pointers are live. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe {
        NetUserGetLocalGroups(
            PCWSTR::null(),
            &account,
            0,
            0,
            &raw mut buffer,
            MAX_PREFERRED_LENGTH,
            &raw mut read,
            &raw mut total,
        )
    };
    let buffer = NetBuffer(buffer);
    if status == NERR_UserNotFound {
        return Ok(Vec::new());
    }
    net("NetUserGetLocalGroups", status)?;
    if buffer.0.is_null() || read == 0 {
        return Ok(Vec::new());
    }
    let count = usize::try_from(read).map_err(|error| MachineError::Other(error.to_string()))?;
    (0..count)
        .map(|index| {
            // SAFETY: level 0 fills `read` LOCALGROUP_USERS_INFO_0 in a row, which live (with
            // their names) until `buffer` is dropped; entry `index` is `index` entries in.
            let entry = unsafe {
                buffer
                    .0
                    .add(index * size_of::<LOCALGROUP_USERS_INFO_0>())
                    .cast::<LOCALGROUP_USERS_INFO_0>()
                    .read_unaligned()
            };
            text(entry.lgrui0_name)
        })
        .collect()
}

/// The local group `name`'s comment; `None` when there is no such group.
fn group_comment(name: &str) -> Result<Option<String>, MachineError> {
    let group = HSTRING::from(name);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    // SAFETY: the name is NUL-terminated; the out-pointer is live. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe { NetLocalGroupGetInfo(PCWSTR::null(), &group, 1, &raw mut buffer) };
    let buffer = NetBuffer(buffer);
    if status == NERR_GroupNotFound || status == ERROR_NO_SUCH_ALIAS.0 {
        return Ok(None);
    }
    net("NetLocalGroupGetInfo", status)?;
    if buffer.0.is_null() {
        return Ok(None);
    }
    // SAFETY: level 1 fills one LOCALGROUP_INFO_1, which lives until `buffer` is dropped; its
    // strings live there too.
    let info = unsafe { buffer.0.cast::<LOCALGROUP_INFO_1>().read_unaligned() };
    text(info.lgrpi1_comment).map(Some)
}

/// The NUL-terminated text at `units`; a null pointer is empty.
fn text(units: PWSTR) -> Result<String, MachineError> {
    if units.is_null() {
        return Ok(String::new());
    }
    // SAFETY: a non-null string the Net* call wrote, NUL-terminated, in its live buffer.
    unsafe { units.to_string() }.map_err(|error| MachineError::Other(error.to_string()))
}

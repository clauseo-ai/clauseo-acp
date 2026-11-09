//! The group reads of the sweep's real [`Sweeper`] (`group.rs`, for the log alone): the record's
//! `GroupSid`, whether a SID is a group's (`LookupAccountSidW`), and a local group's members
//! (`NetLocalGroupGetMembers`, level 0). Each only reads; an error names the call, never a
//! name or a SID.
//!
//! [`Sweeper`]: super::Sweeper
#![expect(unsafe_code, reason = "LookupAccountSidW and NetLocalGroupGetMembers")]

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_NONE_MAPPED};
use windows::Win32::NetworkManagement::NetManagement::{
    LOCALGROUP_MEMBERS_INFO_0, MAX_PREFERRED_LENGTH, NERR_Success, NetLocalGroupGetMembers,
};
use windows::Win32::Security::{
    LookupAccountSidW, SID_NAME_USE, SidTypeAlias, SidTypeGroup, SidTypeWellKnownGroup,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::Failed;
use super::windows_sweeper::lookup_account;
use crate::windows::record::sandbox_group_sid;
use crate::windows::setup::accounts::{LocalSid, NetBuffer};
use crate::windows::setup::preflight::sid_text;
use crate::windows::sids::Sid;

/// The record's `GroupSid`, as [`super::Sweeper::pool_group`] says.
pub(super) fn pool_group() -> Result<Option<Sid>, Failed> {
    let text = sandbox_group_sid()
        .map_err(|error| format!("the record's GroupSid can't be read ({error})"))?;
    let Some(text) = text else {
        return Ok(None);
    };
    Sid::parse(&text)
        .map(Some)
        .map_err(|_not_sid| "the record's GroupSid isn't a SID".to_owned())
}

/// Whether `sid` is a group's, as [`super::Sweeper::is_group`] says.
pub(super) fn is_group(sid: &Sid) -> Result<bool, Failed> {
    let (_, used) = lookup_sid(sid)?;
    Ok([SidTypeGroup, SidTypeAlias, SidTypeWellKnownGroup].contains(&used))
}

/// The members of the local group `sid`, as [`super::Sweeper::group_members`] says.
pub(super) fn group_members(sid: &Sid) -> Result<Vec<Sid>, Failed> {
    let (name, used) = lookup_sid(sid)?;
    if used != SidTypeAlias {
        return Err("the group isn't a local group".to_owned());
    }
    let group = HSTRING::from(name.as_str());
    let mut buffer: *mut u8 = std::ptr::null_mut();
    let (mut read, mut total) = (0u32, 0u32);
    // SAFETY: the name is NUL-terminated and lives for the call; the out-pointers are live. The
    // buffer the call allocates is freed by `NetBuffer`.
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
    let freed = NetBuffer(buffer);
    if status != NERR_Success {
        return Err(format!("NetLocalGroupGetMembers failed ({status})"));
    }
    if read != total {
        return Err("NetLocalGroupGetMembers gave some of the members".to_owned());
    }
    let count = usize::try_from(read).map_err(|error| error.to_string())?;
    let entries: &[LOCALGROUP_MEMBERS_INFO_0] = if count == 0 || freed.0.is_null() {
        &[]
    } else {
        // SAFETY: on success the buffer holds `read` level-0 entries, alive until `freed` drops,
        // which is after the last use of `entries`.
        unsafe { std::slice::from_raw_parts(freed.0.cast(), count) }
    };
    let mut members = Vec::with_capacity(count);
    for entry in entries {
        // SAFETY: each entry's SID is valid while the buffer lives.
        match unsafe { sid_text(entry.lgrmi0_sid) } {
            Ok(Some(member)) => members.push(member),
            Ok(None) => return Err("a member of the group has no SID".to_owned()),
            Err(error) => return Err(error.to_string()),
        }
    }
    // The name was read from the SID; it must name that group still, not one made since.
    if lookup_account(&name)? != *sid {
        return Err("the group's name names another account".to_owned());
    }
    Ok(members)
}

/// The account `sid` names, as its name and its kind (`LookupAccountSidW` on this machine).
fn lookup_sid(sid: &Sid) -> Result<(String, SID_NAME_USE), Failed> {
    let parsed = LocalSid::parse(sid).map_err(|error| error.to_string())?;
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
        Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {}
        Err(error) if error.code() == ERROR_NONE_MAPPED.to_hresult() => {
            return Err("LookupAccountSidW found no such account".to_owned());
        }
        Err(error) => {
            return Err(format!(
                "LookupAccountSidW failed ({:#010x})",
                error.code().0
            ));
        }
        Ok(()) => return Err("LookupAccountSidW gave no size".to_owned()),
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
    .map_err(|error| format!("LookupAccountSidW failed ({:#010x})", error.code().0))?;
    let end = name
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(name.len());
    let name = String::from_utf16(name.get(..end).unwrap_or_default())
        .map_err(|_not_text| "LookupAccountSidW gave a name that isn't text".to_owned())?;
    Ok((name, used))
}

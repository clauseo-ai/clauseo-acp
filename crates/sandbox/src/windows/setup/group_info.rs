//! What set-up reads of the pool's group on Windows, for `WindowsMachine::in_group` and
//! `group_facts`: its members by SID (`NetLocalGroupGetMembers` level 0) and its comment
//! (`NetLocalGroupGetInfo` level 1).
#![expect(unsafe_code, reason = "FFI to the NetLocalGroup calls")]

use windows::Win32::NetworkManagement::NetManagement::{
    LOCALGROUP_INFO_1, LOCALGROUP_MEMBERS_INFO_0, MAX_PREFERRED_LENGTH, NetLocalGroupGetInfo,
    NetLocalGroupGetMembers,
};
use windows::Win32::Security::PSID;
use windows::core::{HSTRING, PCWSTR};

use super::MachineError;
use super::accounts::{NetBuffer, checked_name, net, sid_text};
use crate::windows::sids::Sid;

/// The members of the local group `name`, by SID (`NetLocalGroupGetMembers` level 0).
pub(super) fn group_member_sids(name: &str) -> Result<Vec<Sid>, MachineError> {
    // LOCALGROUP_MEMBERS_INFO_0 is one PSID, so entry `index` is the `index`th pointer.
    const _: () = assert!(size_of::<LOCALGROUP_MEMBERS_INFO_0>() == size_of::<PSID>());
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
    let freed = NetBuffer(buffer);
    net("NetLocalGroupGetMembers", status)?;
    (0..usize::try_from(read).unwrap_or(0))
        .map(|index| {
            // SAFETY: the call wrote `read` entries at `buffer`, which lives until `freed`
            // drops; entry `index` starts `index` pointers in, and holds a valid SID.
            let sid = unsafe {
                freed
                    .0
                    .add(index * size_of::<PSID>())
                    .cast::<PSID>()
                    .read_unaligned()
            };
            sid_text(sid)
        })
        .collect()
}

/// The comment of the local group `name` (`NetLocalGroupGetInfo` level 1); empty for none.
pub(super) fn group_comment(name: &str) -> Result<String, MachineError> {
    checked_name(name)?;
    let group = HSTRING::from(name);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    // SAFETY: the name is NUL-terminated; `buffer` is a live out-pointer. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe { NetLocalGroupGetInfo(PCWSTR::null(), &group, 1, &raw mut buffer) };
    let freed = NetBuffer(buffer);
    net("NetLocalGroupGetInfo", status)?;
    if freed.0.is_null() {
        return Ok(String::new());
    }
    // SAFETY: on success the call wrote one LOCALGROUP_INFO_1 at `buffer`, which lives until
    // `freed` drops.
    let info = unsafe { freed.0.cast::<LOCALGROUP_INFO_1>().read_unaligned() };
    if info.lgrpi1_comment.is_null() {
        return Ok(String::new());
    }
    // SAFETY: as above; the comment is NUL-terminated and lives in the same buffer.
    unsafe { info.lgrpi1_comment.to_string() }
        .map_err(|error| MachineError::Other(error.to_string()))
}

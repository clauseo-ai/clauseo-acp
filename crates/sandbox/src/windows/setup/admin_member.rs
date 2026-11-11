//! Whether an account is a member of the local Administrators group when only its SID is known:
//! the record's owner, for a removal an administrator runs while the owner is away. Read with
//! `NetUserGetLocalGroups` (level 0, `LG_INCLUDE_INDIRECT`, so a domain group in Administrators
//! counts), each local group compared by name with Administrators' name in this Windows'
//! language. Any failure is the `Err`; callers take it as no.
#![expect(unsafe_code, reason = "FFI to NetUserGetLocalGroups")]

use windows::Win32::NetworkManagement::NetManagement::{
    LG_INCLUDE_INDIRECT, LOCALGROUP_USERS_INFO_0, MAX_PREFERRED_LENGTH, NetUserGetLocalGroups,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::MachineError;
use super::accounts::{NetBuffer, lookup_sid, net};
use crate::windows::sids::Sid;

// LOCALGROUP_USERS_INFO_0 is one PWSTR, so entry `index` is the `index`th name pointer.
const _: () = assert!(size_of::<LOCALGROUP_USERS_INFO_0>() == size_of::<PWSTR>());

/// `BUILTIN\Administrators`.
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// Whether the account `sid` is in Administrators, directly or through a group.
pub(crate) fn in_administrators(sid: &Sid) -> Result<bool, MachineError> {
    let admins =
        Sid::parse(ADMINISTRATORS).map_err(|error| MachineError::Other(error.to_string()))?;
    let Some((admins, _, _)) = lookup_sid(&admins)? else {
        return Err(MachineError::Other("Administrators has no name".to_owned()));
    };
    let Some((name, domain, _)) = lookup_sid(sid)? else {
        return Err(MachineError::Other("the account has no name".to_owned()));
    };
    let account = HSTRING::from(format!("{domain}\\{name}"));
    let mut buffer: *mut u8 = std::ptr::null_mut();
    let (mut read, mut total) = (0u32, 0u32);
    // SAFETY: the name is NUL-terminated; the out-pointers are live. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe {
        NetUserGetLocalGroups(
            PCWSTR::null(),
            &account,
            0,
            LG_INCLUDE_INDIRECT,
            &raw mut buffer,
            MAX_PREFERRED_LENGTH,
            &raw mut read,
            &raw mut total,
        )
    };
    let freed = NetBuffer(buffer);
    net("NetUserGetLocalGroups", status)?;
    if read != total {
        return Err(MachineError::Other(
            "the account's groups were cut short".to_owned(),
        ));
    }
    let admins = admins.to_lowercase();
    for index in 0..usize::try_from(read).unwrap_or(0) {
        // SAFETY: the call wrote `read` entries at `buffer`, which lives until `freed` drops;
        // entry `index` starts `index` pointers in and holds a NUL-terminated name.
        let entry = unsafe {
            freed
                .0
                .add(index * size_of::<PWSTR>())
                .cast::<PWSTR>()
                .read_unaligned()
        };
        // SAFETY: as above, the name lives until `freed` drops.
        if unsafe { entry.to_string() }.is_ok_and(|found| found.to_lowercase() == admins) {
            return Ok(true);
        }
    }
    Ok(false)
}

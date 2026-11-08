//! The local accounts and groups whose names start with a prefix, and the `ProfileList` subkeys,
//! for the `elevated_` tests' sweep (`NetUserEnum`, `NetLocalGroupEnum` level 0, `RegEnumKeyExW`).
//! Tests only.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 account and profile enumeration calls"
)]

use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
use windows::Win32::NetworkManagement::NetManagement::{
    FILTER_NORMAL_ACCOUNT, LOCALGROUP_INFO_0, MAX_PREFERRED_LENGTH, NERR_Success,
    NetLocalGroupEnum, NetUserEnum, USER_INFO_0,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, RegEnumKeyExW, RegOpenKeyExW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::MachineError;
use super::accounts::{Kind, NetBuffer};
use super::ffi::PROFILE_LIST;
use super::record_write::{Key, check};

/// The names of this machine's local accounts of kind `kind` that start with `prefix`.
pub(crate) fn names_starting(prefix: &str, kind: Kind) -> Result<Vec<String>, MachineError> {
    let mut buffer: *mut u8 = std::ptr::null_mut();
    let (mut read, mut total) = (0u32, 0u32);
    let (call, status) = match kind {
        // SAFETY: the out-pointers are live; the buffer the call allocates is freed by
        // `NetBuffer` below.
        Kind::User => ("NetUserEnum", unsafe {
            NetUserEnum(
                PCWSTR::null(),
                0,
                FILTER_NORMAL_ACCOUNT,
                &raw mut buffer,
                MAX_PREFERRED_LENGTH,
                &raw mut read,
                &raw mut total,
                None,
            )
        }),
        // SAFETY: as above.
        Kind::Group => ("NetLocalGroupEnum", unsafe {
            NetLocalGroupEnum(
                PCWSTR::null(),
                0,
                &raw mut buffer,
                MAX_PREFERRED_LENGTH,
                &raw mut read,
                &raw mut total,
                None,
            )
        }),
    };
    let freed = NetBuffer(buffer);
    if status != NERR_Success {
        return Err(MachineError::Os { call, code: status });
    }
    let count = usize::try_from(read).unwrap_or(0);
    let mut names = Vec::with_capacity(count);
    for index in 0..count {
        // USER_INFO_0 and LOCALGROUP_INFO_0 are each one PWSTR, so entry `index` is the
        // `index`th name pointer of the array the call wrote.
        const _: () = assert!(size_of::<USER_INFO_0>() == size_of::<PWSTR>());
        const _: () = assert!(size_of::<LOCALGROUP_INFO_0>() == size_of::<PWSTR>());
        // SAFETY: the call wrote `read` entries at `buffer`, which lives until `freed` drops;
        // entry `index` starts `index` pointers in, and holds a NUL-terminated name.
        let entry = unsafe {
            freed
                .0
                .add(index * size_of::<PWSTR>())
                .cast::<PWSTR>()
                .read_unaligned()
        };
        // SAFETY: as above, the name lives until `freed` drops.
        let name =
            unsafe { entry.to_string() }.map_err(|error| MachineError::Other(error.to_string()))?;
        if name.starts_with(prefix) {
            names.push(name);
        }
    }
    Ok(names)
}

/// The subkey names of `HKLM`'s `ProfileList`: one per profile, named by its SID.
pub(crate) fn profile_sids() -> Result<Vec<String>, MachineError> {
    let subkey = HSTRING::from(PROFILE_LIST);
    let mut opened = HKEY::default();
    // SAFETY: the subkey is NUL-terminated and lives for the call; `opened` is a live
    // out-pointer, closed by `Key`.
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            KEY_READ | KEY_WOW64_64KEY,
            &raw mut opened,
        )
    };
    check("RegOpenKeyExW", open)?;
    let key = Key(opened);
    let mut names = Vec::new();
    for index in 0u32.. {
        let mut name = [0u16; 256];
        let mut len = 256u32;
        // SAFETY: `name` is live and `len` units long for the call, and `len` is a live
        // out-pointer; the class and the last-write time are not asked for.
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut len,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        check("RegEnumKeyExW", status)?;
        let units = name
            .get(..usize::try_from(len).unwrap_or(0))
            .unwrap_or_default();
        names.push(String::from_utf16_lossy(units));
    }
    Ok(names)
}

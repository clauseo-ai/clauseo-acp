//! Windows: Smart App Control's policy state, one registry read.
#![expect(unsafe_code, reason = "FFI to the Win32 registry")]

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};
use windows::core::w;

/// `HKLM\SYSTEM\CurrentControlSet\Control\CI\Policy\VerifiedAndReputablePolicyState`, in the
/// 64-bit view; `None` when it is missing, not a DWORD, or can't be read.
pub(super) fn policy_state() -> Option<u32> {
    let mut value = 0u32;
    let mut size = u32::try_from(size_of_val(&value)).ok()?;
    // SAFETY: the subkey and name are NUL-terminated wide literals; `value` is a live u32 and
    // `size` holds its length in bytes, which RRF_RT_REG_DWORD does not write past.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SYSTEM\CurrentControlSet\Control\CI\Policy"),
            w!("VerifiedAndReputablePolicyState"),
            RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY,
            None,
            Some((&raw mut value).cast()),
            Some(&raw mut size),
        )
    };
    (read == ERROR_SUCCESS).then_some(value)
}

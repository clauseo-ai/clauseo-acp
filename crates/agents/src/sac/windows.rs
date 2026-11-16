//! Smart App Control's state, read with `RegGetValueW` (the `windows` crate). One value of one
//! key in `HKEY_LOCAL_MACHINE`, readable by every user; nothing is written.
#![expect(unsafe_code, reason = "FFI to the registry's RegGetValueW")]

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::HSTRING;

use super::SacState;

/// The key holding SAC's state.
const KEY: &str = r"SYSTEM\CurrentControlSet\Control\CI\Policy";
/// Its value: a `REG_DWORD`, 0 off, 1 on, 2 evaluation.
const VALUE: &str = "VerifiedAndReputablePolicyState";

/// SAC's state; [`SacState::Unknown`] with `None` when the value is missing, not a DWORD, or
/// the key does not read.
pub(super) fn read_state() -> SacState {
    let mut state = 0u32;
    let mut size = u32::try_from(size_of::<u32>()).unwrap_or(4);
    // SAFETY: the key and value names are NUL-terminated wide strings that outlive the call;
    // `state` is a valid, writable u32 and `size` holds its size in bytes, as RRF_RT_REG_DWORD
    // requires.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(KEY),
            &HSTRING::from(VALUE),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut state).cast()),
            Some(&raw mut size),
        )
    };
    SacState::from_value((read == ERROR_SUCCESS).then_some(state))
}

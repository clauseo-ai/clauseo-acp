//! The machine access descriptor COM uses now, from
//! `CoGetSystemSecurityPermissions(SD_ACCESSRESTRICTIONS)` (ole32; the `COMSD` value for the
//! machine access limit): `MachineAccessRestriction` when it is set, Windows' built-in
//! descriptor when it isn't. Returned self-relative, so it can be
//! written as the value unchanged in effect. COM is entered on this thread for the call when it
//! isn't already.
#![expect(unsafe_code, reason = "FFI to COM's machine-wide security descriptors")]

use std::ptr::null_mut;

use windows::Win32::Foundation::{HLOCAL, LocalFree, RPC_E_CHANGED_MODE};
use windows::Win32::Security::{
    GetSecurityDescriptorControl, GetSecurityDescriptorLength, MakeSelfRelativeSD,
    PSECURITY_DESCRIPTOR, SE_SELF_RELATIVE,
};
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoGetSystemSecurityPermissions, CoInitializeEx, CoUninitialize,
    SD_ACCESSRESTRICTIONS,
};

use super::seam::StoreError;

/// COM entered on this thread while it lives: left on drop when this entered it; a thread
/// already in another apartment keeps it.
struct Apartment(bool);

impl Apartment {
    /// Joins the multithreaded apartment, or uses the thread's.
    fn enter() -> Result<Apartment, StoreError> {
        // SAFETY: no reserved pointer; undone on drop when it succeeded.
        let entered = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if entered == RPC_E_CHANGED_MODE {
            return Ok(Apartment(false));
        }
        entered
            .ok()
            .map_err(|error| format!("CoInitializeEx failed ({:#010x})", error.code().0))?;
        Ok(Apartment(true))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances this guard's successful CoInitializeEx, on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// The machine access descriptor COM uses now, as self-relative bytes.
pub(super) fn machine_access() -> Result<Vec<u8>, StoreError> {
    let _com = Apartment::enter()?;
    let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
    // SAFETY: `descriptor` is a live out-pointer; what it receives is freed below.
    unsafe { CoGetSystemSecurityPermissions(SD_ACCESSRESTRICTIONS, &raw mut descriptor) }.map_err(
        |error| {
            format!(
                "CoGetSystemSecurityPermissions failed ({:#010x})",
                error.code().0
            )
        },
    )?;
    if descriptor.0.is_null() {
        return Err("CoGetSystemSecurityPermissions gave no descriptor".to_owned());
    }
    let bytes = self_relative(descriptor);
    // SAFETY: allocated with LocalAlloc by COM (its documentation says to free it so), and
    // freed here, once.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    bytes
}

/// `descriptor`'s bytes in self-relative form, made so when it is absolute.
fn self_relative(descriptor: PSECURITY_DESCRIPTOR) -> Result<Vec<u8>, StoreError> {
    let mut control = 0u16;
    let mut revision = 0u32;
    // SAFETY: COM's descriptor is valid; both out-pointers are live.
    unsafe { GetSecurityDescriptorControl(descriptor, &raw mut control, &raw mut revision) }
        .map_err(|error| format!("GetSecurityDescriptorControl failed ({})", error.code().0))?;
    if control & SE_SELF_RELATIVE.0 != 0 {
        // SAFETY: a valid self-relative descriptor; its length is read from its headers.
        let len = unsafe { GetSecurityDescriptorLength(descriptor) };
        let len = usize::try_from(len).unwrap_or(0);
        // SAFETY: the descriptor is `len` bytes long and lives until the caller frees it.
        let bytes = unsafe { std::slice::from_raw_parts(descriptor.0.cast::<u8>(), len) };
        return Ok(bytes.to_vec());
    }
    let mut len = 0u32;
    // SAFETY: no buffer asks only for the size, which the call writes to `len` as it fails.
    let _sized = unsafe { MakeSelfRelativeSD(descriptor, None, &raw mut len) };
    // u64 words, so the descriptor is aligned.
    let mut aligned = vec![0u64; usize::try_from(len).unwrap_or(0).div_ceil(8).max(1)];
    let buffer = PSECURITY_DESCRIPTOR(aligned.as_mut_ptr().cast());
    // SAFETY: `buffer` is live and writable for at least `len` bytes.
    unsafe { MakeSelfRelativeSD(descriptor, Some(buffer), &raw mut len) }
        .map_err(|error| format!("MakeSelfRelativeSD failed ({})", error.code().0))?;
    // SAFETY: the call wrote a self-relative descriptor of `len` bytes into `aligned`.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            aligned.as_ptr().cast::<u8>(),
            usize::try_from(len).unwrap_or(0),
        )
    };
    Ok(bytes.to_vec())
}

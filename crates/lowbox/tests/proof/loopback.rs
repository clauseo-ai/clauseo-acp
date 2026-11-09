//! The loopback exemption (recipe step 4): package A, and only A, joins the list of app
//! containers allowed to reach 127.0.0.1, where the row's proxy listens. The exemption is per
//! package (`NetworkIsolationSetAppContainerConfig` with A's SID), never the machine-wide
//! loopback switch; package B (the probe's) stays off the list. It stands in until PR 4's WFP
//! filters, which let one package reach only its own proxy port; until then the exempt package
//! reaches every 127.0.0.1 listener. The set call replaces the whole list, so the row reads it
//! first, sets it to that list plus A (read back and compared), and at cleanup writes the
//! original back and reads it again to compare.
#![expect(
    unsafe_code,
    reason = "FFI to the NetworkIsolation{Get,Set}AppContainerConfig calls"
)]

use std::ffi::c_void;

use windows::Win32::NetworkManagement::WindowsFirewall::{
    NetworkIsolationGetAppContainerConfig, NetworkIsolationSetAppContainerConfig,
};
use windows::Win32::Security::{GetLengthSid, PSID, SID_AND_ATTRIBUTES};

use super::win::Res;

/// The binary SIDs on the list now.
pub(crate) fn current() -> Res<Vec<Vec<u8>>> {
    let mut count = 0_u32;
    let mut list: *mut SID_AND_ATTRIBUTES = std::ptr::null_mut();
    // SAFETY: both outputs are locals. The list's memory is the API's and is not freed here
    // (its documentation names no free call); the row reads it a few times.
    let rc = unsafe { NetworkIsolationGetAppContainerConfig(&raw mut count, &raw mut list) };
    if rc != 0 {
        return Err(format!(
            "NetworkIsolationGetAppContainerConfig failed with {rc}"
        ));
    }
    let count = usize::try_from(count).unwrap_or(0);
    if list.is_null() {
        return Ok(Vec::new());
    }
    Ok((0..count)
        .map(|i| {
            // SAFETY: the call succeeded, so `list` holds `count` entries with valid SIDs.
            unsafe {
                let sid = (*list.add(i)).Sid;
                let len = usize::try_from(GetLengthSid(sid)).unwrap_or(0);
                std::slice::from_raw_parts(sid.0.cast::<u8>().cast_const(), len).to_vec()
            }
        })
        .collect())
}

/// Replaces the list with `sids`.
pub(crate) fn set(sids: &[Vec<u8>]) -> Res<()> {
    let mut copies: Vec<Vec<u8>> = sids.to_vec();
    let entries: Vec<SID_AND_ATTRIBUTES> = copies
        .iter_mut()
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: PSID(sid.as_mut_ptr().cast::<c_void>()),
            Attributes: 0,
        })
        .collect();
    // SAFETY: each entry points at a binary SID in `copies`, which outlives the call.
    let rc = unsafe { NetworkIsolationSetAppContainerConfig(&entries) };
    if rc != 0 {
        return Err(format!(
            "NetworkIsolationSetAppContainerConfig failed with {rc}"
        ));
    }
    Ok(())
}

/// Sets the list to `original` (just read) plus `package`, then reads it back: it must be
/// exactly `original` plus `package`, with `absent` (package B) not on it. Neither package may
/// be on `original`, since a fresh package never is and the restore would then keep it.
pub(crate) fn exempt(original: &[Vec<u8>], package: &[u8], absent: &[u8]) -> Res<String> {
    if original.iter().any(|sid| sid == package || sid == absent) {
        return Err("a fresh package is already on the loopback list".to_owned());
    }
    let mut want = original.to_vec();
    want.push(package.to_vec());
    set(&want)?;
    let mut now = current()?;
    now.sort();
    want.sort();
    if now != want {
        return Err(format!(
            "the re-read list has {} entries, not the original {} plus package A",
            now.len(),
            original.len()
        ));
    }
    Ok(format!(
        "package A only (re-read: the original {} entries plus A; B is not on it)",
        original.len()
    ))
}

/// Writes `original` back, then reads the list again and compares, entry for entry (sorted: the
/// order is the API's).
pub(crate) fn restore(original: &[Vec<u8>]) -> Res<String> {
    set(original)?;
    let mut now = current()?;
    let mut want = original.to_vec();
    now.sort();
    want.sort();
    if now != want {
        return Err(format!(
            "the re-read list has {} entries, not the original {}",
            now.len(),
            want.len()
        ));
    }
    Ok(format!(
        "restored; the re-read list equals the original ({} entries)",
        want.len()
    ))
}

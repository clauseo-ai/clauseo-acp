//! Windows: whether a recorded Chrome process is gone, and a new one's creation time.
#![expect(
    unsafe_code,
    reason = "FFI to the process APIs, to probe a recorded Chrome without signalling it"
)]

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{ERROR_INVALID_PARAMETER, HANDLE};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::super::parent::windows::started;

/// `STILL_ACTIVE`, the exit code of a process that runs.
const STILL_ACTIVE: u32 = 259;

/// Whether process `pid`, created at `created`, is surely gone: nothing runs under the pid,
/// what does was created at another time, or it has exited. Access denied, or any failure,
/// reads as alive.
pub(super) fn gone(pid: u32, created: u64) -> bool {
    // SAFETY: opening a process by id takes no pointer; the handle is owned below.
    let opened = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) };
    let raw = match opened {
        Ok(raw) => raw,
        // OpenProcess's answer for a pid nothing runs under.
        Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => return true,
        Err(_) => return false,
    };
    // SAFETY: `raw` is the open handle OpenProcess just returned, which nothing else owns; the
    // `OwnedHandle` closes it once, on drop.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let process = HANDLE(handle.as_raw_handle());
    match started(process) {
        Some(time) if time != created => return true,
        None => return false,
        Some(_) => {}
    }
    let mut code = 0_u32;
    // SAFETY: `process` is the live handle `handle` owns, with query rights, and `code` a live
    // u32.
    if unsafe { GetExitCodeProcess(process, &raw mut code) }.is_err() {
        return false;
    }
    code != STILL_ACTIVE
}

/// The creation time of `child`, which runs (or waits suspended) as the guard's child.
pub(crate) fn created(child: &std::process::Child) -> Option<u64> {
    started(HANDLE(child.as_raw_handle()))
}

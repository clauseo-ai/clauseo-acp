//! macOS: every pid (`proc_listallpids`) and a process's arguments (`sysctl KERN_PROCARGS2`),
//! for `scan.rs`. Each call has one wrapper here, with its `unsafe` block and why it is sound.
#![expect(
    unsafe_code,
    reason = "FFI to libproc (proc_listallpids) and sysctl (KERN_ARGMAX, KERN_PROCARGS2)"
)]

use std::ffi::{c_int, c_void};

use nix::libc;

/// How many pids the first `proc_listallpids` call makes room for; a full buffer is retried with
/// twice the room, up to [`MAX_PIDS`].
const FIRST_PIDS: usize = 1_024;
/// The most processes listed; a system with more fails the listing.
const MAX_PIDS: usize = 1 << 20;

/// Every pid but 0 and 1; `None` when the call fails, answers 0 (as xnu answers a failed system
/// call), or more than [`MAX_PIDS`] run.
pub(super) fn all_pids() -> Option<Vec<i32>> {
    let mut room = FIRST_PIDS;
    loop {
        let mut pids: Vec<c_int> = vec![0; room];
        let bytes = c_int::try_from(pids.len() * size_of::<c_int>()).ok()?;
        // SAFETY: `pids` is a live, zeroed buffer of exactly `bytes` bytes that the call may fill
        // with pids; the kernel writes at most `bytes` bytes. The return counts pids, or is -1 on
        // failure, which `usize::try_from` refuses; a count is used only capped at the buffer's
        // length, so no read passes it.
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let count = usize::try_from(count).ok().filter(|&count| count > 0)?;
        if count < room {
            pids.truncate(count);
            return Some(pids.into_iter().filter(|&pid| pid > 1).collect());
        }
        if room >= MAX_PIDS {
            return None;
        }
        room *= 2;
    }
}

/// `pid`'s argument area as `KERN_PROCARGS2` gives it: its argument count, its executable's
/// path, then its arguments and environment, each ended by a NUL. `None` when the call fails
/// (the process is gone, a zombie, or another user's).
pub(super) fn args_of(pid: i32) -> Option<Vec<u8>> {
    let mut buffer = vec![0_u8; arg_max()?];
    let mut size = buffer.len();
    let mut name = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // SAFETY: `name` is a live array of the 3 integers passed as its length; `buffer` is a live
    // buffer of `size` bytes, and `size` a live length the kernel lowers to what it wrote; no new
    // value is set (null, 0).
    let result = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast::<c_void>(),
            &raw mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return None;
    }
    buffer.truncate(size.min(buffer.len()));
    Some(buffer)
}

/// `KERN_ARGMAX`: the size of the largest argument area.
fn arg_max() -> Option<usize> {
    let mut value: c_int = 0;
    let mut size = size_of::<c_int>();
    let mut name = [libc::CTL_KERN, libc::KERN_ARGMAX];
    // SAFETY: `name` is a live array of the 2 integers passed as its length; `value` is a live
    // `c_int` of `size` bytes, which the kernel writes at most; no new value is set (null, 0).
    let result = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            2,
            (&raw mut value).cast::<c_void>(),
            &raw mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return None;
    }
    usize::try_from(value).ok().filter(|&max| max > 0)
}

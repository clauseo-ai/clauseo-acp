//! macOS: the memory of the app and the `WebKit` processes working for it, through libproc
//! (`proc_listallpids`, `proc_name`, `proc_pid_rusage`, public since macOS 10.9) and
//! `responsibility_get_pid_responsible_for_pid` (libSystem, private: not in the SDK headers;
//! Activity Monitor groups processes by it). The marks build only, never shipped.
//!
//! `WebKit`'s `WebContent`, Networking and GPU processes are XPC services that launchd starts: they
//! are not our children, and their parent is pid 1. What ties them to the app is their
//! responsible process, which launchd sets from the client that asked for the service. A process
//! counts when all three hold:
//!
//! - its name starts with `com.apple.WebKit.` (`com.apple.WebKit.WebContent`, `.Networking`,
//!   `.GPU`); the agent processes the host starts share our responsible process but are not
//!   `WebKit`, so they are left out;
//! - its responsible process is the app, or the app's own responsible process (the shell, the
//!   terminal or the runner that started the app, when it was not opened from the Finder);
//! - it started after the app (`ri_proc_start_abstime`), so a `WebKit` process of an earlier run
//!   that outlived it is left out.
//!
//! Assumption: no other app that uses `WebKit` is started after the app by the same parent (its
//! `WebKit` processes would count). The scoreboard's macOS runner starts nothing else.

#![expect(
    unsafe_code,
    reason = "libproc and responsibility calls, which the crate has no binding for"
)]

use std::ffi::{c_int, c_void};
use std::io;

/// `RUSAGE_INFO_V4` (`<sys/resource.h>`).
const RUSAGE_INFO_V4: c_int = 4;

/// The place of `ri_phys_footprint` among `rusage_info_v4`'s counters.
const PHYS_FOOTPRINT: usize = 7;

/// The place of `ri_proc_start_abstime` among `rusage_info_v4`'s counters.
const PROC_START_ABSTIME: usize = 8;

/// The name prefix of `WebKit`'s helper processes.
const WEBKIT_PREFIX: &[u8] = b"com.apple.WebKit.";

/// `struct rusage_info_v4` (`<sys/resource.h>`): a uuid, then 35 `uint64_t` counters.
#[repr(C)]
struct RusageInfoV4 {
    _uuid: [u8; 16],
    counters: [u64; 35],
}

unsafe extern "C" {
    fn proc_listallpids(buffer: *mut c_void, buffersize: c_int) -> c_int;
    fn proc_name(pid: c_int, buffer: *mut c_void, buffersize: u32) -> c_int;
    fn proc_pid_rusage(pid: c_int, flavor: c_int, buffer: *mut c_void) -> c_int;
    fn responsibility_get_pid_responsible_for_pid(pid: c_int) -> c_int;
}

/// A process's memory and start.
#[derive(Debug, Clone, Copy)]
struct Usage {
    /// `phys_footprint`, in bytes.
    footprint: u64,
    /// `ri_proc_start_abstime`, in mach absolute time.
    start: u64,
}

/// `pid`'s usage, or the OS error (the process ended, or is not ours to read).
fn usage(pid: c_int) -> io::Result<Usage> {
    let mut info = RusageInfoV4 {
        _uuid: [0; 16],
        counters: [0; 35],
    };
    // SAFETY: `info` has the layout of `rusage_info_v4`, which the V4 flavor writes and no more.
    let done = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V4, (&raw mut info).cast()) };
    if done != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Usage {
        footprint: info.counters[PHYS_FOOTPRINT],
        start: info.counters[PROC_START_ABSTIME],
    })
}

/// Every pid on the machine.
fn all_pids() -> io::Result<Vec<c_int>> {
    let mut capacity = 4096;
    loop {
        let mut pids: Vec<c_int> = vec![0; capacity];
        let bytes = c_int::try_from(capacity * size_of::<c_int>())
            .map_err(|_overflow| io::Error::other("too many processes"))?;
        // SAFETY: `pids` holds `bytes` bytes; the call writes at most that many.
        let count = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let Ok(count) = usize::try_from(count) else {
            return Err(io::Error::last_os_error());
        };
        if count < capacity {
            pids.truncate(count);
            return Ok(pids);
        }
        capacity *= 2;
    }
}

/// Whether `pid`'s name starts with [`WEBKIT_PREFIX`].
fn is_webkit(pid: c_int) -> bool {
    let mut name = [0_u8; 256];
    // SAFETY: `name` holds 256 bytes; the call writes at most that many.
    let len = unsafe { proc_name(pid, name.as_mut_ptr().cast(), 256) };
    usize::try_from(len)
        .ok()
        .and_then(|len| name.get(..len))
        .is_some_and(|name| name.starts_with(WEBKIT_PREFIX))
}

/// The process responsible for `pid`, `None` when the call fails.
fn responsible(pid: c_int) -> Option<c_int> {
    // SAFETY: a pid in, a pid (or -1) out; the call only reads the kernel's process table.
    let found = unsafe { responsibility_get_pid_responsible_for_pid(pid) };
    (found > 0).then_some(found)
}

/// The bytes of `phys_footprint` of this process and of every `WebKit` process working for it
/// (the module docs say which those are).
pub(super) fn group_footprint() -> io::Result<u64> {
    let me = c_int::try_from(std::process::id())
        .map_err(|_overflow| io::Error::other("the pid does not fit a c_int"))?;
    let mine = usage(me)?;
    // Our own responsible process, unless it is us or launchd.
    let parent = responsible(me).filter(|&parent| parent != me && parent > 1);
    let mut total = mine.footprint;
    for pid in all_pids()? {
        if pid == me || !is_webkit(pid) {
            continue;
        }
        let Some(owner) = responsible(pid) else {
            continue;
        };
        if owner != me && Some(owner) != parent {
            continue;
        }
        // One that ended since the list was read is not counted.
        if let Ok(theirs) = usage(pid)
            && theirs.start >= mine.start
        {
            total += theirs.footprint;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_has_a_footprint_and_is_not_webkit() {
        let me = c_int::try_from(std::process::id()).unwrap();
        let mine = usage(me).unwrap();
        assert!(mine.footprint > 0);
        assert!(mine.start > 0);
        assert!(!is_webkit(me));
        assert!(all_pids().unwrap().contains(&me));
        // No `WebKit` process works for a test binary, so the group is this process alone
        // (give or take what it allocated between the two reads).
        let group = group_footprint().unwrap();
        assert!(group >= mine.footprint / 2, "{group} {}", mine.footprint);
        assert!(responsible(me).is_some());
    }
}

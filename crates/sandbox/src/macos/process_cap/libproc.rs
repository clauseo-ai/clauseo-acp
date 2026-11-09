//! The process cap's system calls, each with its `unsafe` block and the reason it is sound: the
//! limits (`sysctlbyname`, `getrlimit`), libproc's listing and reads (`proc_listallpids`,
//! `proc_pidinfo`), `getsid`, and the signals (`kill`, `killpg`). [`Libproc`] is the
//! watchdog's [`System`] on them.
#![expect(
    unsafe_code,
    reason = "FFI: sysctlbyname, getrlimit, libproc, getsid, kill and killpg for the process cap"
)]

use std::ffi::{CStr, c_int, c_void};
use std::io;
use std::time::Duration;

use super::Proc;
use super::watchdog::System;

/// `SZOMB` from `<sys/proc.h>`: `pbi_status` of a process that has exited and waits to be
/// reaped. The libc crate does not export it.
const SZOMB: u32 = 5;

/// How many pids a listing's first call makes room for; a full buffer is retried with twice the
/// room, up to [`MAX_PIDS`].
const FIRST_PIDS: usize = 1_024;
/// The most pids a listing reads; a system with more fails it.
const MAX_PIDS: usize = 1 << 20;

/// `kern.maxprocperuid`: the most processes one user may run.
pub(crate) fn per_user() -> io::Result<u64> {
    sysctl_int(c"kern.maxprocperuid")
}

/// `kern.maxproc`: the most processes the system runs, every user's together.
pub(crate) fn max_proc() -> io::Result<u64> {
    sysctl_int(c"kern.maxproc")
}

/// The `int` sysctl `name`, which must not be negative.
fn sysctl_int(name: &CStr) -> io::Result<u64> {
    let mut value: c_int = 0;
    let mut size = size_of::<c_int>();
    // SAFETY: the name is a NUL-terminated string; `value` is a live `c_int` and `size` says
    // its size, so the kernel writes at most that many bytes; no new value is set (null, 0).
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&raw mut value).cast::<c_void>(),
            &raw mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    u64::try_from(value).map_err(io::Error::other)
}

/// This process's hard `RLIMIT_NPROC`.
pub(crate) fn hard_limit() -> io::Result<u64> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a live `rlimit` the call fills.
    if unsafe { libc::getrlimit(libc::RLIMIT_NPROC, &raw mut limit) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(limit.rlim_max)
}

/// Every pid, zombies included; `None` when the listing fails.
fn pids() -> Option<Vec<u32>> {
    listing(|buffer, bytes| {
        // SAFETY: `buffer` is a live, zeroed buffer of exactly `bytes` bytes that the call may
        // fill with pids and nothing else. The return counts pids (xnu
        // libsyscall/wrappers/libproc/libproc.c divides the byte count by `sizeof(int)`) or is
        // -1 on failure.
        unsafe { libc::proc_listallpids(buffer, bytes) }
    })
}

/// The session of `pid`; `None` when it is gone or a zombie (`getsid` answers `ESRCH` for
/// both, measured). Any user's process answers: xnu's `getsid` checks no permission.
fn session_of(pid: u32) -> Option<u32> {
    let raw = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: getsid(2) takes a plain integer and answers a session id or -1.
    let sid = unsafe { libc::getsid(raw) };
    u32::try_from(sid).ok()
}

/// The start of running process `pid`; `None` when it is gone, unreadable or a zombie.
fn start_of(pid: u32) -> Option<u64> {
    info(pid).map(|process| process.start)
}

/// Sends `signal` to `pid` if it still runs with `start` (so a reused pid is never signalled).
/// Returns whether it went.
fn signal(pid: u32, start: u64, signal: c_int) -> bool {
    let Ok(raw) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if raw <= 1 || start_of(pid) != Some(start) {
        return false;
    }
    // SAFETY: kill(2) takes plain integers; `raw` is a single process, never 0, -1 or launchd.
    unsafe { libc::kill(raw, signal) == 0 }
}

/// Sends `signal` to process group `group`. Returns whether it went.
fn signal_group(group: u32, signal: c_int) -> bool {
    let Ok(raw) = libc::pid_t::try_from(group) else {
        return false;
    };
    if raw <= 1 {
        return false;
    }
    // SAFETY: killpg(2) takes plain integers; `raw` names one group, never 0 (this process's).
    unsafe { libc::killpg(raw, signal) == 0 }
}

/// Fills a buffer of pids by `call(buffer, bytes)`, which answers how many it wrote or -1;
/// a full buffer is retried with twice the room. `None` on a failure or past [`MAX_PIDS`].
fn listing(call: impl Fn(*mut c_void, c_int) -> c_int) -> Option<Vec<u32>> {
    let mut room = FIRST_PIDS;
    loop {
        let mut pids: Vec<c_int> = vec![0; room];
        let bytes = c_int::try_from(pids.len() * size_of::<c_int>()).ok()?;
        let count = usize::try_from(call(pids.as_mut_ptr().cast(), bytes)).ok()?;
        if count < room {
            pids.truncate(count);
            // Unused slots stay 0, which is never a process the cap reads.
            return Some(
                pids.into_iter()
                    .filter_map(|pid| u32::try_from(pid).ok().filter(|&pid| pid != 0))
                    .collect(),
            );
        }
        if room >= MAX_PIDS {
            return None;
        }
        room *= 2;
    }
}

/// `proc_pidinfo(PROC_PIDTBSDINFO)` and `getsid` for `pid`, as a [`Proc`]; `None` when the
/// call fails (another user's process, or one gone), answers short (a zombie answers 0), or the
/// process is a zombie or gone by the `getsid`.
fn info(pid: u32) -> Option<Proc> {
    let raw = c_int::try_from(pid).ok()?;
    let mut bsd = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: `bsd` is a live buffer of exactly `size` bytes, the size of `proc_bsdinfo`, the
    // struct the `PROC_PIDTBSDINFO` flavour fills; the call writes at most `size` bytes. The
    // return is the byte count written, and anything but `size` is refused before the struct
    // is read.
    let written = unsafe {
        libc::proc_pidinfo(
            raw,
            libc::PROC_PIDTBSDINFO,
            0,
            bsd.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the call wrote the whole struct (`written == size`), and it was zeroed before;
    // `proc_bsdinfo` is plain integers and arrays.
    let bsd = unsafe { bsd.assume_init() };
    if bsd.pbi_status == SZOMB {
        return None;
    }
    Some(Proc {
        pid,
        parent: bsd.pbi_ppid,
        session: session_of(pid)?,
        start: bsd
            .pbi_start_tvsec
            .saturating_mul(1_000_000)
            .saturating_add(bsd.pbi_start_tvusec),
        uid: bsd.pbi_ruid,
    })
}

/// The watchdog's [`System`]: this Mac's processes, through the calls above.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Libproc;

impl System for Libproc {
    fn pids(&self) -> Option<Vec<u32>> {
        pids()
    }

    fn session_of(&self, pid: u32) -> Option<u32> {
        session_of(pid)
    }

    fn info(&self, pid: u32) -> Option<Proc> {
        info(pid)
    }

    fn signal(&self, pid: u32, start: u64, signal: c_int) -> bool {
        self::signal(pid, start, signal)
    }

    fn signal_group(&self, group: u32, signal: c_int) -> bool {
        signal_group(group, signal)
    }

    fn pause(&self, time: Duration) {
        std::thread::park_timeout(time);
    }
}

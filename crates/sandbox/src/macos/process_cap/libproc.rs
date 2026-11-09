//! The process cap's system calls, each with its `unsafe` block and the reason it is sound: the
//! limits (`sysctlbyname`, `getrlimit`), libproc's listings and reads (`proc_listpgrppids`,
//! `proc_listallpids`, `proc_pidinfo`), the signals (`kill`, `killpg`) and the agent's kqueue.
#![expect(
    unsafe_code,
    reason = "FFI: sysctlbyname, getrlimit, libproc, kill, killpg and kqueue for the process cap"
)]

use std::ffi::{c_int, c_void};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;

use super::Proc;

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
    let mut value: c_int = 0;
    let mut size = size_of::<c_int>();
    // SAFETY: the name is a NUL-terminated literal; `value` is a live `c_int` and `size` says
    // its size, so the kernel writes at most that many bytes; no new value is set (null, 0).
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.maxprocperuid".as_ptr(),
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

/// How many pids are in process group `group`, zombies included; `None` when the call fails.
pub(crate) fn group_size(group: u32) -> Option<usize> {
    let group = c_int::try_from(group).ok()?;
    listing(|buffer, bytes| {
        // SAFETY: `buffer` is a live, zeroed buffer of exactly `bytes` bytes that the call may
        // fill with pids and nothing else. The return counts pids (xnu
        // libsyscall/wrappers/libproc/libproc.c divides the byte count by `sizeof(int)`) or is
        // -1 on failure.
        unsafe { libc::proc_listpgrppids(group, buffer, bytes) }
    })
    .map(|pids| pids.len())
}

/// Every running process this user can read: its pid, parent, group and start. Zombies and
/// processes `proc_pidinfo` refuses (another user's) are left out. `None` when the listing
/// fails.
pub(crate) fn all() -> Option<Vec<Proc>> {
    let pids = listing(|buffer, bytes| {
        // SAFETY: as in `group_size`: a live buffer of `bytes` bytes, filled with pids only;
        // the return counts pids or is -1.
        unsafe { libc::proc_listallpids(buffer, bytes) }
    })?;
    Some(pids.into_iter().filter_map(info).collect())
}

/// The start of running process `pid`; `None` when it is gone, unreadable or a zombie.
pub(crate) fn start_of(pid: u32) -> Option<u64> {
    info(pid).map(|process| process.start)
}

/// Sends `signal` to `pid` if it still runs with `start` (so a reused pid is never signalled).
/// Returns whether it went.
pub(crate) fn signal(pid: u32, start: u64, signal: c_int) -> bool {
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
pub(crate) fn signal_group(group: u32, signal: c_int) -> bool {
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

/// `proc_pidinfo(PROC_PIDTBSDINFO)` for `pid`, as a [`Proc`]; `None` when the call fails,
/// answers short, or the process is a zombie.
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
    (bsd.pbi_status != SZOMB).then(|| Proc {
        pid,
        parent: bsd.pbi_ppid,
        group: bsd.pbi_pgid,
        start: bsd
            .pbi_start_tvsec
            .saturating_mul(1_000_000)
            .saturating_add(bsd.pbi_start_tvusec),
    })
}

/// Why [`Queue::wait`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wake {
    /// The time ran out, or a signal broke the wait.
    Tick,
    /// The agent forked.
    Fork,
    /// The agent exited.
    Exit,
    /// The queue failed.
    Failed,
}

/// A kqueue watching the agent's forks and exit (`EVFILT_PROC`, `NOTE_FORK | NOTE_EXIT`).
/// `NOTE_FORK` comes for the agent's own forks only: its children are not followed
/// (`NOTE_TRACK` is gone since 10.5), so it wakes the count early and the poll does the rest.
#[derive(Debug)]
pub(crate) struct Queue(OwnedFd);

impl Queue {
    /// A queue on `pid`; `None` when the process is gone already.
    pub(crate) fn new(pid: u32) -> io::Result<Option<Queue>> {
        let ident = usize::try_from(pid).map_err(io::Error::other)?;
        // SAFETY: kqueue(2) takes no argument and answers a new descriptor or -1.
        let fd = unsafe { libc::kqueue() };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is a new descriptor this process owns and nothing else holds.
        let queue = Queue(unsafe { OwnedFd::from_raw_fd(fd) });
        let change = libc::kevent {
            ident,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: libc::NOTE_FORK | libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: one change from a live struct, no event list (null, 0), no timeout: the call
        // registers the filter and returns.
        let status = unsafe {
            libc::kevent(
                queue.0.as_raw_fd(),
                &raw const change,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if status < 0 {
            let error = io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(libc::ESRCH) => Ok(None),
                _ => Err(error),
            };
        }
        Ok(Some(queue))
    }

    /// Waits up to `timeout` for the agent's next fork or its exit.
    pub(crate) fn wait(&self, timeout: Duration) -> Wake {
        let Ok(tv_sec) = libc::time_t::try_from(timeout.as_secs()) else {
            return Wake::Failed;
        };
        let timeout = libc::timespec {
            tv_sec,
            tv_nsec: libc::c_long::from(timeout.subsec_nanos()),
        };
        let mut event = libc::kevent {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: no changes (null, 0); one event slot in a live struct; a live timeout. The
        // call writes at most one event.
        let count = unsafe {
            libc::kevent(
                self.0.as_raw_fd(),
                std::ptr::null(),
                0,
                &raw mut event,
                1,
                &raw const timeout,
            )
        };
        match count {
            0 => Wake::Tick,
            1 if event.flags & libc::EV_ERROR != 0 => Wake::Failed,
            1 if event.fflags & libc::NOTE_EXIT != 0 => Wake::Exit,
            1 => Wake::Fork,
            _ if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) => Wake::Tick,
            _ => Wake::Failed,
        }
    }
}

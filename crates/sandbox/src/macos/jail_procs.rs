//! Every process of one jailed session, found by its Seatbelt profile rather than by its
//! ancestry (decision record 0038). A child that calls `setsid` leaves the agent's process
//! group, and one whose parent exits is re-parented to launchd, so neither the group nor the
//! tree finds it; but no process can shed the sandbox it was started in, and every child
//! inherits it.
//!
//! So each launch passes its profile a name of its own, `SESSION_PROBE`, and the profile allows
//! a `mach-lookup` of that one name (nothing is ever registered under it). A process belongs to
//! the session when `sandbox_check` says it is sandboxed, may look up the session's name, and
//! may not look up [`CONTROL`], a name no profile allows: no process outside a Seatbelt jail is
//! sandboxed, and no other launch's profile allows this launch's name. The checks pass
//! `SANDBOX_CHECK_NO_REPORT`, so they log no denial. One pass over every process
//! (`proc_listallpids`) takes about 2 ms for 700 processes on an M-series Mac.
//!
//! Each libproc and libsandbox call has one wrapper here, with its `unsafe` block.
#![expect(
    unsafe_code,
    reason = "FFI to libproc (proc_listallpids, proc_pidinfo) and libsandbox (sandbox_check)"
)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};

use rustix::process::{Pid, Signal, kill_process};

use super::profile::PROBE_PREFIX as PREFIX;

/// A name no profile allows: a process that may look it up is not jailed by Clauseo.
const CONTROL: &CStr = c"chat.clauseo.session.none";

/// `SANDBOX_FILTER_NONE` and `SANDBOX_FILTER_GLOBAL_NAME` (`<sandbox.h>`'s private enum).
const FILTER_NONE: c_int = 0;
const FILTER_GLOBAL_NAME: c_int = 2;

/// `PROC_PIDTBSDINFO` (`<sys/proc_info.h>`).
const PROC_PIDTBSDINFO: c_int = 3;

/// `SZOMB` (`<sys/proc.h>`): a process that has exited and waits to be reaped.
const SZOMB: u32 = 5;

/// How many pids the first listing makes room for; a full one is retried with twice the room.
const FIRST_PIDS: usize = 2_048;

/// The most processes a listing reads.
const MAX_PIDS: usize = 1 << 20;

unsafe extern "C" {
    fn proc_listallpids(buffer: *mut c_void, buffersize: c_int) -> c_int;
    fn proc_pidinfo(
        pid: c_int,
        flavor: c_int,
        arg: u64,
        buffer: *mut c_void,
        buffersize: c_int,
    ) -> c_int;
    fn sandbox_check(pid: c_int, operation: *const c_char, filter: c_int, ...) -> c_int;
    static SANDBOX_CHECK_NO_REPORT: c_int;
}

/// `struct proc_bsdinfo` (`<sys/proc_info.h>`), 136 bytes.
#[repr(C)]
#[derive(Default)]
struct BsdInfo {
    flags: u32,
    status: u32,
    xstatus: u32,
    pid: u32,
    ppid: u32,
    ids: [u32; 7],
    comm: [u8; 16],
    name: [u8; 32],
    nfiles: u32,
    pgid: u32,
    pjobc: u32,
    tdev: u32,
    tpgid: u32,
    nice: i32,
    start_sec: u64,
    start_usec: u64,
}

/// One session's probe: the name its profile allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Probe {
    name: String,
    c_name: CString,
}

impl Probe {
    /// The probe of launch `n` of the launcher tagged `tag` (`clauseo-<pid>-<counter>`).
    pub(crate) fn new(tag: &str, n: u64) -> Probe {
        Probe::named(format!("{PREFIX}{tag}-{n}"))
    }

    /// A probe by its full name, as a record holds it; `None` for one Clauseo never made.
    pub(crate) fn from_name(name: &str) -> Option<Probe> {
        let rest = name.strip_prefix(PREFIX)?;
        let fits = !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        fits.then(|| Probe::named(name.to_owned()))
    }

    fn named(name: String) -> Probe {
        // The name is ASCII letters, digits, `.` and `-`: never a NUL.
        let c_name = CString::new(name.clone()).unwrap_or_default();
        Probe { name, c_name }
    }

    /// The name, for the profile's parameter and the records.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The running processes of the session, zombies left out; `None` when the processes
    /// can't be listed.
    pub(crate) fn pids(&self) -> Option<Vec<i32>> {
        Some(
            all_pids()?
                .into_iter()
                .filter(|&pid| self.holds(pid))
                .collect(),
        )
    }

    /// Whether the session has no process left; a listing that fails counts as not yet.
    pub(crate) fn gone(&self) -> bool {
        self.pids().is_some_and(|pids| pids.is_empty())
    }

    /// Sends SIGKILL to every process of the session, each checked again just before its
    /// signal, so a pid reused since the listing is left alone. Returns how many were sent.
    pub(crate) fn kill_all(&self) -> usize {
        let Some(pids) = self.pids() else {
            tracing::error!(probe = %self.name, "the jailed processes could not be listed");
            return 0;
        };
        let mut sent = 0;
        for pid in pids {
            if self.holds(pid)
                && let Some(target) = Pid::from_raw(pid)
                && kill_process(target, Signal::KILL).is_ok()
            {
                sent += 1;
            }
        }
        sent
    }

    /// Whether `pid` is a running process of this session.
    fn holds(&self, pid: i32) -> bool {
        sandboxed(pid)
            && may_look_up(pid, &self.c_name) == Some(true)
            && may_look_up(pid, CONTROL) == Some(false)
            && info(pid).is_some_and(|info| info.status != SZOMB)
    }
}

/// Whether `pid` runs in a Seatbelt sandbox.
fn sandboxed(pid: i32) -> bool {
    // SAFETY: a null operation and no filter argument is `sandbox_check`'s documented "is this
    // process sandboxed" form; reading the exported constant is a plain load.
    let answer = unsafe { sandbox_check(pid, std::ptr::null(), FILTER_NONE | no_report()) };
    answer == 1
}

/// Whether the sandbox of `pid` lets it look up the Mach service `name`: `Some(false)` when it
/// is denied, `None` when the check fails.
fn may_look_up(pid: i32, name: &CStr) -> Option<bool> {
    // SAFETY: the operation and the name are NUL-terminated strings that outlive the call, and
    // the one variadic argument is the `const char *` the global-name filter reads.
    let answer = unsafe {
        sandbox_check(
            pid,
            c"mach-lookup".as_ptr(),
            FILTER_GLOBAL_NAME | no_report(),
            name.as_ptr(),
        )
    };
    match answer {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

/// `SANDBOX_CHECK_NO_REPORT`: a check that logs no denial.
fn no_report() -> c_int {
    // SAFETY: a constant libsandbox exports; reading it is a plain load.
    unsafe { SANDBOX_CHECK_NO_REPORT }
}

/// Every pid on the system; `None` when the listing fails.
fn all_pids() -> Option<Vec<i32>> {
    let mut room = FIRST_PIDS;
    while room <= MAX_PIDS {
        let mut pids = vec![0_i32; room];
        let bytes = c_int::try_from(room * size_of::<i32>()).ok()?;
        // SAFETY: the buffer holds `bytes` bytes, and the call writes at most that many.
        let count = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let count = usize::try_from(count).ok()?;
        if count < room {
            pids.truncate(count);
            pids.retain(|&pid| pid > 0);
            return Some(pids);
        }
        room *= 2;
    }
    None
}

/// `proc_pidinfo(PROC_PIDTBSDINFO)` of `pid`; `None` when it is gone or unreadable.
fn info(pid: i32) -> Option<BsdInfo> {
    let mut info = BsdInfo::default();
    let size = c_int::try_from(size_of::<BsdInfo>()).ok()?;
    // SAFETY: the buffer is a `proc_bsdinfo` of `size` bytes, which the call fills.
    let got = unsafe { proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    (got == size).then_some(info)
}

/// The start of running process `pid` in microseconds since the epoch, which with the pid
/// names one process; `None` when it is gone, unreadable or a zombie.
pub(crate) fn start_of(pid: u32) -> Option<u64> {
    let info = info(i32::try_from(pid).ok()?).filter(|info| info.status != SZOMB)?;
    info.start_sec
        .checked_mul(1_000_000)?
        .checked_add(info.start_usec)
}

/// This process's pid and start, for the records of its sessions.
pub(crate) fn this_process() -> (u32, Option<u64>) {
    let pid = std::process::id();
    (pid, start_of(pid))
}

#[cfg(test)]
mod tests;

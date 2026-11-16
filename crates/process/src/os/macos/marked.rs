//! A session's sweep on macOS (`session.rs`): every pid from `proc_listallpids`, each one's
//! real and effective uid, state and start from `proc_pidinfo(PROC_PIDTBSDINFO)` (which answers
//! only for this user's processes), its environment from `sysctl(KERN_PROCARGS2)` (which answers
//! for this user's processes that are not protected; any failure skips the process). macOS has
//! no pidfd, so a process is read again, start, uid and entry, as the last step before `kill(2)`
//! by pid.
#![expect(
    unsafe_code,
    reason = "sysctl(KERN_ARGMAX) and sysctl(KERN_PROCARGS2), to read a process's environment"
)]

use std::ffi::{c_int, c_void};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use super::{Read, SZOMB, all_pids, read_bsd};
use crate::os::procargs::environment;
use crate::session::carries;
use crate::session::passes::Table;

/// The largest argument and environment area read for one process, should `KERN_ARGMAX` fail.
const ARGMAX_FALLBACK: usize = 1 << 20;

/// The processes of this machine, as this user's sweep reads and ends them.
#[derive(Debug, Clone)]
pub(crate) struct Marked {
    uid: u32,
    own: u32,
}

impl Marked {
    /// The table for this process: its real uid and its pid.
    pub(crate) fn new() -> Marked {
        Marked {
            uid: nix::unistd::getuid().as_raw(),
            own: std::process::id(),
        }
    }

    /// The start of `pid` when it runs and both its uids are this process's real uid.
    fn own_start(&self, pid: u32) -> Option<u64> {
        let Read::Got(bsd) = read_bsd(pid) else {
            return None;
        };
        let ours = bsd.pbi_uid == self.uid && bsd.pbi_ruid == self.uid;
        (ours && bsd.pbi_status != SZOMB).then(|| {
            bsd.pbi_start_tvsec
                .saturating_mul(1_000_000)
                .saturating_add(bsd.pbi_start_tvusec)
        })
    }
}

impl Table for Marked {
    type Pin = ();

    fn pids(&self) -> Option<Vec<u32>> {
        all_pids()
    }

    fn own(&self) -> u32 {
        self.own
    }

    fn marked(&self, pid: u32, entry: &[u8]) -> Option<u64> {
        let start = self.own_start(pid)?;
        let buffer = procargs(pid)?;
        if !carries(environment(&buffer)?.into_iter(), entry) {
            return None;
        }
        // The same process throughout: a pid reused during the reads has a later start.
        (self.own_start(pid)? == start).then_some(start)
    }

    fn pin(&self, _pid: u32) -> Option<()> {
        Some(())
    }

    fn signal(&self, pid: u32, _pin: &()) -> bool {
        i32::try_from(pid).is_ok_and(|raw| kill(Pid::from_raw(raw), Signal::SIGKILL).is_ok())
    }

    fn runs(&self, pid: u32, start: u64) -> bool {
        super::start_of(pid) == Some(start)
    }
}

/// The size of the argument and environment area, `KERN_ARGMAX`, or [`ARGMAX_FALLBACK`].
fn argmax() -> usize {
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let mut value: c_int = 0;
    let mut size = size_of::<c_int>();
    // SAFETY: `mib` names a two-level `int` value; `value` is a live `c_int` and `size` holds
    // its exact size, so the kernel writes at most that many bytes into it; no new value is set
    // (null pointer, length 0).
    let done = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&raw mut value).cast::<c_void>(),
            &raw mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if done == 0 && size == size_of::<c_int>() {
        usize::try_from(value)
            .ok()
            .filter(|&max| max > 0)
            .unwrap_or(ARGMAX_FALLBACK)
    } else {
        ARGMAX_FALLBACK
    }
}

/// The `KERN_PROCARGS2` area of `pid`: its argument count, executable path, arguments and
/// environment; `None` when the call fails (another user's process, a protected one, a
/// zombie, a pid that is gone).
fn procargs(pid: u32) -> Option<Vec<u8>> {
    let pid = c_int::try_from(pid).ok()?;
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut buffer = vec![0_u8; argmax()];
    let mut size = buffer.len();
    // SAFETY: `mib` names one process's argument area; `buffer` is a live, zeroed buffer of
    // exactly `size` bytes, and the kernel writes at most `size` bytes into it and sets `size`
    // to the count written; no new value is set (null pointer, length 0).
    let done = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast::<c_void>(),
            &raw mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if done != 0 {
        return None;
    }
    buffer.truncate(size);
    Some(buffer)
}

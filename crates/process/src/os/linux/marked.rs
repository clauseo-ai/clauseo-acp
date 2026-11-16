//! A session's sweep on Linux (`session.rs`): every pid from `/proc`, each one's real and
//! effective uid from `/proc/<pid>/status`, its environment from `/proc/<pid>/environ` (readable
//! for this user's processes, empty for a zombie), its start from `/proc/<pid>/stat`. A process
//! is pinned with `pidfd_open(2)` before it is read the last time and signalled through the pidfd
//! (`pidfd_send_signal(2)`), so the SIGKILL reaches the process that was read or none, even if
//! its pid was reused in between. A kernel without pidfds (before 5.3) or a seccomp filter that
//! refuses them falls back to `kill(2)` by pid right after that read.
#![expect(
    unsafe_code,
    reason = "the pidfd_open and pidfd_send_signal system calls, which nix does not wrap"
)]

use std::ffi::c_int;
use std::fs;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use super::start_of;
use crate::session::passes::Table;
use crate::session::{carries, entries};

/// The processes of this machine, as this user's sweep reads and ends them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Procfs {
    uid: u32,
    own: u32,
}

impl Procfs {
    /// The table for this process: its real uid and its pid.
    pub(crate) fn new() -> Procfs {
        Procfs {
            uid: nix::unistd::getuid().as_raw(),
            own: std::process::id(),
        }
    }
}

/// How a process is held between its last read and its signal.
#[derive(Debug)]
pub(crate) enum Held {
    /// Its pidfd: a signal through it reaches this process or fails.
    Fd(OwnedFd),
    /// No pidfd on this system: the signal goes by pid.
    Pid,
}

impl Table for Procfs {
    type Pin = Held;

    fn pids(&self) -> Option<Vec<u32>> {
        let pids = fs::read_dir("/proc")
            .ok()?
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .collect();
        Some(pids)
    }

    fn own(&self) -> u32 {
        self.own
    }

    fn marked(&self, pid: u32, entry: &[u8]) -> Option<u64> {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        if uids(&status)? != (self.uid, self.uid) {
            return None;
        }
        let start = start_of(pid)?;
        let environ = fs::read(format!("/proc/{pid}/environ")).ok()?;
        if !carries(entries(&environ), entry) {
            return None;
        }
        // The same process throughout: a pid reused during the reads has a later start.
        (start_of(pid)? == start).then_some(start)
    }

    fn pin(&self, pid: u32) -> Option<Held> {
        let raw = c_int::try_from(pid).ok()?;
        // SAFETY: pidfd_open(2) takes a pid and a flags word (0) and returns a new file
        // descriptor or -1 with errno set; it reads and writes no memory of this process.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, raw, 0) };
        if fd < 0 {
            return match Errno::last() {
                // No pidfds here: the kernel predates them, or a seccomp filter refuses them.
                Errno::ENOSYS | Errno::EPERM => Some(Held::Pid),
                _ => None,
            };
        }
        let fd = c_int::try_from(fd).ok()?;
        // SAFETY: `fd` is a descriptor pidfd_open just opened for this process, owned by
        // nothing else; the `OwnedFd` closes it once.
        Some(Held::Fd(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    fn signal(&self, pid: u32, pin: &Held) -> bool {
        match pin {
            Held::Fd(fd) => {
                // SAFETY: pidfd_send_signal(2) takes a live pidfd (owned by `pin`, open for the
                // whole call), a signal number, a null `siginfo_t` pointer (the kernel then fills
                // one as `kill` does) and a flags word (0); it writes no memory of this process.
                let sent = unsafe {
                    libc::syscall(
                        libc::SYS_pidfd_send_signal,
                        fd.as_raw_fd(),
                        libc::SIGKILL,
                        std::ptr::null::<libc::siginfo_t>(),
                        0,
                    )
                };
                sent == 0
            }
            Held::Pid => i32::try_from(pid)
                .is_ok_and(|raw| kill(Pid::from_raw(raw), Signal::SIGKILL).is_ok()),
        }
    }

    fn runs(&self, pid: u32, start: u64) -> bool {
        start_of(pid) == Some(start)
    }
}

/// The real and effective uid on a `/proc/<pid>/status` text's `Uid:` line (real, effective,
/// saved, file system); `None` when it has none or it does not parse.
pub(crate) fn uids(status: &str) -> Option<(u32, u32)> {
    let line = status.lines().find_map(|line| line.strip_prefix("Uid:"))?;
    let mut ids = line.split_ascii_whitespace().map(str::parse::<u32>);
    Some((ids.next()?.ok()?, ids.next()?.ok()?))
}

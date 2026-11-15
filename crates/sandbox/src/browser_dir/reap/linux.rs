//! Linux: the listener from `SO_PEERCRED`, held by a pidfd; its executable from
//! `/proc/<pid>/exe`; signals through the pidfd, and its exit seen as the pidfd turning readable.

use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::process::{Pid, PidfdFlags, Signal};

/// The process listening on a socket, held by a pidfd opened while the connection is open.
pub(super) struct Process {
    pid: Pid,
    uid: u32,
    pidfd: OwnedFd,
}

impl Process {
    /// The listener at the other end of `stream`: its pid and user id from `SO_PEERCRED`, and a
    /// pidfd for that pid. The caller checks afterwards that the connection has not hung up, so
    /// the pidfd names the listener and not a later process given the same pid.
    pub(super) fn listening_on(stream: &UnixStream) -> io::Result<Process> {
        let cred = rustix::net::sockopt::socket_peercred(stream)?;
        let pidfd = rustix::process::pidfd_open(cred.pid, PidfdFlags::empty())?;
        Ok(Process {
            pid: cred.pid,
            uid: cred.uid.as_raw(),
            pidfd,
        })
    }

    /// Its pid.
    pub(super) fn pid(&self) -> i32 {
        self.pid.as_raw_nonzero().get()
    }

    /// Its user id when it listened.
    pub(super) fn uid(&self) -> u32 {
        self.uid
    }

    /// Its executable's real path, from `/proc/<pid>/exe`.
    pub(super) fn executable(&self) -> io::Result<PathBuf> {
        std::fs::read_link(format!("/proc/{}/exe", self.pid()))
    }

    /// Sends `signal` through the pidfd.
    pub(super) fn signal(&self, signal: Signal) -> Result<(), rustix::io::Errno> {
        rustix::process::pidfd_send_signal(&self.pidfd, signal)
    }

    /// Whether it exits within `wait`: its pidfd turns readable.
    pub(super) fn wait_gone(&self, _stream: &UnixStream, wait: Duration) -> bool {
        let mut fds = [PollFd::new(&self.pidfd, PollFlags::IN)];
        let timeout = Timespec::try_from(wait).ok();
        matches!(rustix::event::poll(&mut fds, timeout.as_ref()), Ok(n) if n > 0)
    }
}

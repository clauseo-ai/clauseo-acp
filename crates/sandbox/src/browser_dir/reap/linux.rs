//! Linux: the listener's pid and user from `SO_PEERCRED`, held by a pidfd the connection itself
//! gives (`SO_PEERPIDFD`, kernel 6.5 and later) or, on an older kernel, one `pidfd_open` makes
//! for that pid; its executable from `/proc/<pid>/exe`; signals through the pidfd, and its exit
//! seen as the pidfd turning readable.

#![expect(
    unsafe_code,
    reason = "getsockopt(SO_PEERPIDFD), which rustix has no binding for, and the fd it gives"
)]

use std::io;
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::process::{Pid, PidfdFlags, Signal};

/// The process listening on a socket, held by a pidfd taken while the connection is open.
pub(super) struct Process {
    pid: Pid,
    uid: u32,
    pidfd: OwnedFd,
}

impl Process {
    /// The listener at the other end of `stream`: its pid and user id from `SO_PEERCRED`, and
    /// its pidfd from `SO_PEERPIDFD`, which names the listening process itself. On a kernel
    /// without it, `pidfd_open` of the pid: the caller then checks that the connection has not
    /// hung up, so that pidfd names the listener and not a later process given the same pid.
    pub(super) fn listening_on(stream: &UnixStream) -> io::Result<Process> {
        let cred = rustix::net::sockopt::socket_peercred(stream)?;
        let pidfd = match peer_pidfd(stream) {
            Ok(pidfd) => pidfd,
            Err(error)
                if matches!(error.raw_os_error(), Some(libc::ENOPROTOOPT | libc::EINVAL)) =>
            {
                rustix::process::pidfd_open(cred.pid, PidfdFlags::empty())?
            }
            Err(error) => return Err(error),
        };
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

/// The pidfd of `stream`'s peer, from `getsockopt(SO_PEERPIDFD)`.
fn peer_pidfd(stream: &UnixStream) -> io::Result<OwnedFd> {
    let mut fd: libc::c_int = -1;
    let mut len = libc::socklen_t::try_from(size_of::<libc::c_int>()).map_err(io::Error::other)?;
    // SAFETY: `stream` holds its socket open for the call; `fd` and `len` are live locals of the
    // size `len` says, which the call writes at most.
    let got = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERPIDFD,
            (&raw mut fd).cast(),
            &raw mut len,
        )
    };
    if got != 0 {
        return Err(io::Error::last_os_error());
    }
    if fd < 0 {
        return Err(io::Error::other("no peer pidfd"));
    }
    // SAFETY: the kernel just made `fd`, a new descriptor this process owns and nothing else
    // holds; the `OwnedFd` closes it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

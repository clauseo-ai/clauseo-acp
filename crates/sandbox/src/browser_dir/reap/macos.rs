//! macOS: the listener from `LOCAL_PEERPID` and `getpeereid`; its executable from libproc's
//! `proc_pidpath`; signals through `kill`, and its exit seen as the connection hanging up.

#![expect(
    unsafe_code,
    reason = "getsockopt(LOCAL_PEERPID), getpeereid and proc_pidpath, which rustix has no binding for"
)]

use std::ffi::OsString;
use std::io;
use std::os::fd::AsRawFd as _;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use rustix::process::{Pid, Signal};

/// libproc's `PROC_PIDPATHINFO_MAXSIZE`: the most bytes `proc_pidpath` writes.
const PATH_MAX_SIZE: usize = 4 * 1024;

/// The process listening on a socket, read while the connection is open.
pub(super) struct Process {
    pid: Pid,
    uid: u32,
}

impl Process {
    /// The listener at the other end of `stream`: its pid (`LOCAL_PEERPID`) and its effective
    /// user id (`getpeereid`).
    pub(super) fn listening_on(stream: &UnixStream) -> io::Result<Process> {
        let fd = stream.as_raw_fd();
        let mut pid: libc::c_int = 0;
        let mut len =
            libc::socklen_t::try_from(size_of::<libc::c_int>()).map_err(io::Error::other)?;
        // SAFETY: `fd` is the open socket `stream` holds for the call; `pid` and `len` are live
        // locals of the size `len` says, which the call writes at most.
        let got = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&raw mut pid).cast(),
                &raw mut len,
            )
        };
        if got != 0 {
            return Err(io::Error::last_os_error());
        }
        let pid = Pid::from_raw(pid).ok_or_else(|| io::Error::other("no listener pid"))?;
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        // SAFETY: `fd` is the open socket `stream` holds for the call; `uid` and `gid` are live
        // locals the call writes.
        if unsafe { libc::getpeereid(fd, &raw mut uid, &raw mut gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Process { pid, uid })
    }

    /// Its pid.
    pub(super) fn pid(&self) -> i32 {
        self.pid.as_raw_nonzero().get()
    }

    /// Its effective user id when it listened.
    pub(super) fn uid(&self) -> u32 {
        self.uid
    }

    /// Its executable's real path, from `proc_pidpath`.
    pub(super) fn executable(&self) -> io::Result<PathBuf> {
        let mut buffer = vec![0u8; PATH_MAX_SIZE];
        let size = u32::try_from(buffer.len()).map_err(io::Error::other)?;
        // SAFETY: `buffer` is a live allocation of `size` bytes, the most the call writes.
        let written = unsafe { libc::proc_pidpath(self.pid(), buffer.as_mut_ptr().cast(), size) };
        let Ok(written) = usize::try_from(written) else {
            return Err(io::Error::last_os_error());
        };
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(written);
        Ok(PathBuf::from(OsString::from_vec(buffer)))
    }

    /// Sends `signal` with `kill`.
    pub(super) fn signal(&self, signal: Signal) -> Result<(), rustix::io::Errno> {
        rustix::process::kill_process(self.pid, signal)
    }

    /// Whether it exits within `wait`: the connection it listened on hangs up, or (checked
    /// after) no process has its pid any more.
    pub(super) fn wait_gone(&self, stream: &UnixStream, wait: Duration) -> bool {
        super::hung_up(stream, wait)
            || rustix::process::test_kill_process(self.pid) == Err(rustix::io::Errno::SRCH)
    }
}

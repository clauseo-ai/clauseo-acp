//! Other Unix systems: the listener cannot be identified, so no daemon is ended.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use rustix::process::Signal;

/// Never made: [`Process::listening_on`] always fails here.
pub(super) struct Process {
    pid: i32,
}

impl Process {
    /// Always an error: no listener lookup on this system.
    pub(super) fn listening_on(_stream: &UnixStream) -> io::Result<Process> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// Its pid.
    pub(super) fn pid(&self) -> i32 {
        self.pid
    }

    /// Never read.
    pub(super) fn uid(&self) -> u32 {
        u32::MAX
    }

    /// Never read.
    pub(super) fn executable(&self) -> io::Result<PathBuf> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// Never sent.
    pub(super) fn signal(&self, _signal: Signal) -> Result<(), rustix::io::Errno> {
        Err(rustix::io::Errno::NOSYS)
    }

    /// Never waited on.
    pub(super) fn wait_gone(&self, _stream: &UnixStream, _wait: Duration) -> bool {
        false
    }
}

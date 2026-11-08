//! The denial pipe on Windows: a runner pipe's server end ([`Role::Denials`], inbound) under
//! [`denial_sddl`] for the desktop user, named in a fresh random set; the outer opened by its
//! pid for its creation time, and so the wait for a client ends when it exits. That open runs
//! on the launching thread while the host's `Launched` holds the outer's process handle, so the
//! pid can't have been reused (async-process exposes no handle to duplicate). The client's pid
//! comes from the pipe, and its creation time is read the same way (`pipe::os::open_process`).
//! Each read also waits on the outer's handle (`ServerPipe::read_until`): its exit ends the feed.

use std::io;
use std::time::Duration;

use super::{DenialPipe, Got, Peer};
use crate::windows::pipe::os::{Opened, ServerPipe, open_process};
use crate::windows::pipe::{PipeSet, Role, denial_sddl};

/// A launch's denial pipe, and the outer once watched: what the real steps' `create_pipe`
/// gives (C5).
pub struct OsDenialPipe {
    pipe: ServerPipe,
    outer: Option<Opened>,
}

impl OsDenialPipe {
    /// A new pipe, and its full name for the launch file: one instance, made first or refused,
    /// local clients only, the desktop user (this process's) alone.
    pub fn create() -> io::Result<(OsDenialPipe, String)> {
        let name = PipeSet::random()?.name(Role::Denials);
        let sddl = denial_sddl(&crate::windows::record::current_user_sid()?);
        let pipe = ServerPipe::create(&name, Role::Denials, &sddl)?;
        Ok((OsDenialPipe { pipe, outer: None }, name))
    }
}

impl DenialPipe for OsDenialPipe {
    fn watch(&mut self, pid: u32) -> io::Result<Peer> {
        let outer = open_process(pid)?;
        let started = outer.started;
        self.outer = Some(outer);
        Ok(Peer { pid, started })
    }

    fn accept(&mut self, within: Duration) -> io::Result<Option<Peer>> {
        let watch = self
            .outer
            .as_ref()
            .map(Opened::handle)
            .ok_or_else(|| io::Error::other("the outer isn't watched"))?;
        match self.pipe.connect(watch, within) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::BrokenPipe
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
        let pid = self.pipe.client_pid()?;
        let started = open_process(pid)?.started;
        Ok(Some(Peer { pid, started }))
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<Got> {
        let watch = self
            .outer
            .as_ref()
            .map(Opened::handle)
            .ok_or_else(|| io::Error::other("the outer isn't watched"))?;
        Ok(match self.pipe.read_until(buf, watch)? {
            None => Got::Exited,
            Some(0) => Got::Closed,
            Some(read) => Got::Bytes(read),
        })
    }
}

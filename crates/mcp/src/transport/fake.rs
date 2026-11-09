//! Tests only: a [`Connection`] over one end of a socket pair, whose peer check gives the answer
//! the test chose, so both ends' refusals of a stranger can be tested as the same user.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::{Connection, Peer};

/// One end of a socket pair, with the peer check's answer fixed.
pub(crate) struct Fake {
    pub(crate) stream: UnixStream,
    /// What every peer check answers.
    pub(crate) passes: bool,
}

impl Read for Fake {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }
}

impl Write for Fake {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl Connection for Fake {
    fn try_clone(&self) -> io::Result<Self> {
        Ok(Fake {
            stream: self.stream.try_clone()?,
            passes: self.passes,
        })
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.stream.shutdown(how)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_write_timeout(timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    fn peer_is(&self, _peer: Peer) -> bool {
        self.passes
    }
}

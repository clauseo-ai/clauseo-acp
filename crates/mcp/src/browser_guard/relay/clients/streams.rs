//! The client's connection as the relay reads and writes it, each with a bound on its time:
//!
//! - [`ToClient`] writes each whole line within [`WRITE_WAIT`], however the client reads it. A line
//!   not written by then, or a write that fails, shuts the connection (so its reads end, which is
//!   the client's disconnect) and every later write fails at once: the agent's output lock is never
//!   held past that bound.
//! - [`FirstLine`] gives a new client [`FIRST_LINE_WAIT`] for its first whole line. One that has
//!   not sent it by then is taken as gone, so a connection that sends nothing cannot hold the one
//!   client's place.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::time::{Duration, Instant};

use crate::browser_guard::{note, now};
use crate::transport::Connection;

/// The longest one line to the client may take to write.
pub(crate) const WRITE_WAIT: Duration = Duration::from_secs(10);
/// The longest a new client may take to send its first whole line.
pub(crate) const FIRST_LINE_WAIT: Duration = Duration::from_secs(10);
/// Tests only (the `test-support` feature, never in a shipped build): [`WRITE_WAIT`] in
/// milliseconds, read from the guard's own environment.
#[cfg(feature = "test-support")]
const WRITE_WAIT_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_WRITE_MS";
/// Tests only (the `test-support` feature): [`FIRST_LINE_WAIT`] in milliseconds.
#[cfg(feature = "test-support")]
const FIRST_LINE_WAIT_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_FIRST_LINE_MS";

/// The milliseconds the test's variable `knob` gives, if any (a `test-support` build only).
#[cfg(feature = "test-support")]
fn knob(knob: &str) -> Option<Duration> {
    let ms = std::env::var(knob).ok()?.parse::<u64>().ok()?;
    Some(Duration::from_millis(ms))
}

/// [`WRITE_WAIT`], or in a `test-support` build the test's.
pub(crate) fn write_wait() -> Duration {
    #[cfg(feature = "test-support")]
    if let Some(wait) = knob(WRITE_WAIT_FOR_TESTS) {
        return wait;
    }
    WRITE_WAIT
}

/// [`FIRST_LINE_WAIT`], or in a `test-support` build the test's.
pub(crate) fn first_line_wait() -> Duration {
    #[cfg(feature = "test-support")]
    if let Some(wait) = knob(FIRST_LINE_WAIT_FOR_TESTS) {
        return wait;
    }
    FIRST_LINE_WAIT
}

/// The client's side of [`Out`](super::super::lines::Out).
pub(crate) struct ToClient<C: Connection> {
    conn: C,
    wait: Duration,
    /// Set at the first failure: the connection is shut and nothing more is written.
    dead: bool,
}

impl<C: Connection> ToClient<C> {
    pub(crate) fn new(conn: C, wait: Duration) -> ToClient<C> {
        ToClient {
            conn,
            wait,
            dead: false,
        }
    }

    /// Shuts the connection for good and returns `error`.
    fn fail(&mut self, error: io::Error) -> io::Error {
        self.dead = true;
        drop(self.conn.shutdown(Shutdown::Both));
        error
    }
}

impl<C: Connection> Write for ToClient<C> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_all(buf).map(|()| buf.len())
    }

    /// All of `buf` within the wait, each write bounded by what is left of it.
    fn write_all(&mut self, mut buf: &[u8]) -> io::Result<()> {
        if self.dead {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let deadline = now() + self.wait;
        while !buf.is_empty() {
            let left = deadline.saturating_duration_since(now());
            if left.is_zero() {
                return Err(self.fail(io::ErrorKind::TimedOut.into()));
            }
            if let Err(error) = self.conn.set_write_timeout(Some(left)) {
                return Err(self.fail(error));
            }
            match self.conn.write(buf) {
                Ok(0) => return Err(self.fail(io::ErrorKind::WriteZero.into())),
                Ok(written) => buf = buf.get(written..).unwrap_or_default(),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(self.fail(error)),
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.dead {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let flushed = self.conn.flush();
        flushed.map_err(|error| self.fail(error))
    }
}

/// The client's side of the agent's input: reads bounded by a deadline until the first newline.
pub(crate) struct FirstLine<C: Connection> {
    conn: C,
    /// `None` once the first line has arrived.
    deadline: Option<Instant>,
}

impl<C: Connection> FirstLine<C> {
    pub(crate) fn new(conn: C, wait: Duration) -> FirstLine<C> {
        FirstLine {
            conn,
            deadline: Some(now() + wait),
        }
    }
}

impl<C: Connection> Read for FirstLine<C> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(deadline) = self.deadline else {
            return self.conn.read(buf);
        };
        let left = deadline.saturating_duration_since(now());
        let read = if left.is_zero() {
            Err(io::ErrorKind::TimedOut.into())
        } else {
            self.conn
                .set_read_timeout(Some(left))
                .and_then(|()| self.conn.read(buf))
        };
        match read {
            Ok(count) => {
                if buf.get(..count).is_some_and(|bytes| bytes.contains(&b'\n')) {
                    self.deadline = None;
                    self.conn.set_read_timeout(None)?;
                }
                Ok(count)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                note("closed a client that sent no whole line in time");
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

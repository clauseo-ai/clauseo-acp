//! The client's connection as the relay reads and writes it, each with a bound on its time:
//!
//! - [`ToClient`] writes each whole line within [`WRITE_WAIT`], however the client reads it. A line
//!   not written by then, or a write that fails, shuts the connection (so its reads end, which is
//!   the client's disconnect) and every later write fails at once: the agent's output lock is never
//!   held past that bound.
//! - [`FirstLine`] gives a new client [`FIRST_LINE_WAIT`] for its first whole line, which must be
//!   a JSON-RPC request or notification, and then [`IDLE_WAIT`] from each whole line to the next,
//!   paused while a request of its awaits its answer. One that sends no such line in time, sends
//!   anything else first, or later sends no whole line for the idle wait, is taken as gone, so no
//!   connection holds the one client's place by sending nothing, noise or a byte at a time.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::time::{Duration, Instant};

use crate::browser_guard::filter::MAX_LINE_BYTES;
use crate::browser_guard::{note, now};
use crate::transport::Connection;

/// The longest one line to the client may take to write.
pub(crate) const WRITE_WAIT: Duration = Duration::from_secs(10);
/// The longest a new client may take, from the accept, to send its first whole line.
pub(crate) const FIRST_LINE_WAIT: Duration = Duration::from_secs(30);
/// The longest a client may go without sending a whole line once its first line has passed, with
/// none of its requests awaiting an answer: the browser's own idle stop (`session::IDLE_STOP`),
/// so a quiet client is closed no sooner than its browser would stop anyway. Its disconnect stops
/// the browser as the idle stop does, and the bridge connects again at its next request. Windows'
/// pipe (`bjail-wj1-pipe`) has no idle bound of its own to match.
pub(crate) const IDLE_WAIT: Duration = Duration::from_secs(600);
/// Tests only (the `test-support` feature, never in a shipped build): [`WRITE_WAIT`] in
/// milliseconds, read from the guard's own environment.
#[cfg(feature = "test-support")]
const WRITE_WAIT_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_WRITE_MS";
/// Tests only (the `test-support` feature): [`FIRST_LINE_WAIT`] in milliseconds.
#[cfg(feature = "test-support")]
const FIRST_LINE_WAIT_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_FIRST_LINE_MS";
/// Tests only (the `test-support` feature): [`IDLE_WAIT`] in milliseconds.
#[cfg(feature = "test-support")]
const IDLE_WAIT_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_CLIENT_IDLE_MS";

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

/// [`IDLE_WAIT`], or in a `test-support` build the test's.
pub(crate) fn idle_wait() -> Duration {
    #[cfg(feature = "test-support")]
    if let Some(wait) = knob(IDLE_WAIT_FOR_TESTS) {
        return wait;
    }
    IDLE_WAIT
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

/// The client's side of the agent's input, with two deadlines, each a point in time that only a
/// whole line moves: partial bytes never extend either.
///
/// - Until the first newline: [`FIRST_LINE_WAIT`] from the accept. What arrives is kept here,
///   and the line must then be a request or notification (a JSON object with a string
///   `method`), or the connection is refused, so a bare newline or other noise does not take
///   the one client's place. The kept bytes are then handed on as read.
/// - After it: [`IDLE_WAIT`] from the last newline that arrived. While any of the client's
///   requests awaits its answer ([`FirstLine::paused_while`]), the deadline does not run: it
///   starts again, whole, once the last answer is in, so a slow tool call is never cut off.
///
/// Each read's timeout is the time left until the deadline, set afresh before every read, so a
/// client dripping one byte at a time is dropped at the deadline all the same.
pub(crate) struct FirstLine<C: Connection> {
    conn: C,
    /// When the client is taken as gone, unless a whole line arrives (or, after the first line,
    /// a request of its awaits its answer).
    deadline: Instant,
    /// The bound after each whole line.
    idle: Duration,
    /// Whether the first line has arrived and passed.
    passed: bool,
    /// What was read up to the first newline and past it in the same read, not yet handed on.
    kept: Vec<u8>,
    /// How much of `kept` has been scanned for the first newline: each byte is scanned once.
    scanned: usize,
    /// How much of `kept` has been handed on.
    handed: usize,
    /// Whether any of the client's requests awaits its answer; the deadline waits meanwhile.
    busy: Option<Box<dyn Fn() -> bool + Send>>,
    /// Whether `busy` said so at the last look.
    was_busy: bool,
}

/// How often a read looks again whether the client's requests are all answered, while one
/// awaits its answer.
const BUSY_LOOK: Duration = Duration::from_secs(1);

impl<C: Connection> FirstLine<C> {
    /// `wait` from now for the first line, then `idle` after each whole line.
    pub(crate) fn new(conn: C, wait: Duration, idle: Duration) -> FirstLine<C> {
        FirstLine {
            conn,
            deadline: now() + wait,
            idle,
            passed: false,
            kept: Vec::new(),
            scanned: 0,
            handed: 0,
            busy: None,
            was_busy: false,
        }
    }

    /// Holds the idle deadline while `busy` says a request of the client's awaits its answer.
    #[must_use]
    pub(crate) fn paused_while(mut self, busy: impl Fn() -> bool + Send + 'static) -> FirstLine<C> {
        self.busy = Some(Box::new(busy));
        self
    }

    /// One read from the connection, bounded by the deadline (by [`BUSY_LOOK`] instead while a
    /// request awaits its answer, the deadline then moving with it). `Ok(None)` when the bound
    /// passed with the deadline still ahead: the caller reads again. `Err` once the deadline
    /// has passed.
    fn read_bounded(&mut self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        let busy = self.passed && self.busy.as_ref().is_some_and(|busy| busy());
        // While busy, and once at the end of it, the whole idle bound lies ahead again.
        if busy || self.was_busy {
            self.deadline = now() + self.idle;
        }
        self.was_busy = busy;
        let left = self.deadline.saturating_duration_since(now());
        if left.is_zero() {
            note(if self.passed {
                "closed a client that sent no whole line for the idle wait"
            } else {
                "closed a client that sent no whole line in time"
            });
            return Err(io::ErrorKind::TimedOut.into());
        }
        let bound = if busy { left.min(BUSY_LOOK) } else { left };
        self.conn.set_read_timeout(Some(bound))?;
        match self.conn.read(buf) {
            Ok(count) => Ok(Some(count)),
            Err(error) if timed_out(&error) || error.kind() == io::ErrorKind::Interrupted => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Reads until the first newline, within the first deadline, and checks the line it ends.
    fn first_line(&mut self) -> io::Result<()> {
        let mut chunk = [0_u8; 8192];
        let end = loop {
            let unscanned = self.kept.get(self.scanned..).unwrap_or_default();
            if let Some(at) = unscanned.iter().position(|&byte| byte == b'\n') {
                break self.scanned + at;
            }
            self.scanned = self.kept.len();
            if self.kept.len() > MAX_LINE_BYTES {
                return Err(refused("closed a client whose first line is too long"));
            }
            match self.read_bounded(&mut chunk)? {
                None => {}
                Some(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Some(count) => self
                    .kept
                    .extend_from_slice(chunk.get(..count).unwrap_or_default()),
            }
        };
        if !is_request(self.kept.get(..end).unwrap_or_default()) {
            return Err(refused("closed a client whose first line is not a request"));
        }
        self.passed = true;
        self.deadline = now() + self.idle;
        Ok(())
    }
}

/// Whether `line` is a JSON-RPC request or notification: a JSON object with a string `method`.
fn is_request(line: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(line).is_ok_and(|message| {
        message
            .get("method")
            .is_some_and(serde_json::Value::is_string)
    })
}

fn timed_out(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Notes `why` and returns the error that ends the client.
fn refused(why: &str) -> io::Error {
    note(why);
    io::ErrorKind::InvalidData.into()
}

impl<C: Connection> Read for FirstLine<C> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.passed {
            self.first_line()?;
        }
        if let Some(rest) = self.kept.get(self.handed..)
            && !rest.is_empty()
        {
            let count = rest.len().min(buf.len());
            buf.get_mut(..count)
                .unwrap_or_default()
                .copy_from_slice(rest.get(..count).unwrap_or_default());
            self.handed += count;
            if self.handed == self.kept.len() {
                self.kept = Vec::new();
                self.handed = 0;
            }
            return Ok(count);
        }
        loop {
            if let Some(count) = self.read_bounded(buf)? {
                // Each byte read is scanned once, here.
                if buf.get(..count).is_some_and(|bytes| bytes.contains(&b'\n')) {
                    self.deadline = now() + self.idle;
                }
                return Ok(count);
            }
        }
    }
}

#[cfg(test)]
mod tests;

//! The relay socket, read and written without blocking in the kernel. One descriptor serves both
//! threads (no `try_clone`), it is non-blocking, and the wait between two tries is a wait on a
//! channel: the stdin side ends a read at once by dropping its sender, and the handshake's
//! deadline is a timer thread that drops one. Nothing here relies on `shutdown` to wake a read,
//! and no read timeout (`SO_RCVTIMEO`) is ever set, so no socket is used after one fired.
//!
//! Why: on Windows a `shutdown` from one thread does not complete a `recv` blocked in another
//! (Winsock documents only `closesocket` as cancelling blocking calls), and a blocking receive
//! that timed out leaves "the connection in an indeterminate state" (`setsockopt`,
//! `SO_RCVTIMEO`). A reader blocked in `recv` on a link the mode has ended keeps the socket open
//! until the relay acts, and a relay waiting for the end never does.

use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;

/// The `shutdown` the mode uses. Tests give one that does nothing, the Windows model.
pub(crate) type Shut = fn(&TcpStream, Shutdown) -> io::Result<()>;

/// What ends a read or a write that is waiting: the drop of this receiver's sender. A message on
/// it is a nudge instead: the reader's wait goes back to [`FIRST_WAIT`] and it tries at once, as
/// the stdin side has just forwarded a request and a reply may follow soon.
pub(crate) type Stop = Receiver<()>;

/// The wait after a try that found the socket not ready; each later one doubles up to
/// [`IDLE_WAIT`], and data resets it.
const FIRST_WAIT: Duration = Duration::from_millis(1);
/// The longest wait between two tries of an idle socket: the most a reply waits to be read.
const IDLE_WAIT: Duration = Duration::from_millis(20);

/// A [`Stop`] that fires by itself `after` this long, for the handshake's deadline: a timer
/// thread holds its sender and drops it then. Dropping the returned guard ends the timer at once,
/// and the stop fires with it; by then the handshake is over and nothing waits on it.
pub(crate) fn stop_after(after: Duration) -> (mpsc::Sender<()>, Stop) {
    let (fire, stop) = mpsc::channel::<()>();
    let (guard, cancelled) = mpsc::channel::<()>();
    thread::spawn(move || {
        if cancelled.recv_timeout(after).is_err() {
            // The time is up, or the guard is gone: either way the sender drops now.
        }
        drop(fire);
    });
    (guard, stop)
}

/// A reader on the relay socket that never blocks in `recv`: it tries, then waits on its stop.
pub(crate) struct Polled {
    socket: Arc<TcpStream>,
    stop: Stop,
    wait: Duration,
}

impl Polled {
    /// A reader on `socket`, set non-blocking here, whose reads fail once `stop` fires.
    pub(crate) fn new(socket: Arc<TcpStream>, stop: Stop) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            stop,
            wait: FIRST_WAIT,
        })
    }

    /// From now on a read ends when `stop` fires, not the one given before.
    pub(crate) fn stop_on(&mut self, stop: Stop) {
        self.stop = stop;
    }

    /// The socket, for a best-effort `shutdown` when the reader is done.
    pub(crate) fn socket(&self) -> &TcpStream {
        &self.socket
    }

    /// `Err` once the stop has fired. Nudges waiting on it are taken, each resetting the wait.
    fn check(&mut self) -> io::Result<()> {
        loop {
            match self.stop.try_recv() {
                Ok(()) => self.wait = FIRST_WAIT,
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(stopped()),
            }
        }
    }

    /// Waits before the next try: the current wait, then doubled up to [`IDLE_WAIT`]. A nudge
    /// ends it at once and resets it to [`FIRST_WAIT`]; `Err` once the stop fires.
    fn pause(&mut self) -> io::Result<()> {
        match self.stop.recv_timeout(self.wait) {
            Ok(()) => self.wait = FIRST_WAIT,
            Err(RecvTimeoutError::Timeout) => {
                self.wait = self.wait.saturating_mul(2).min(IDLE_WAIT);
            }
            Err(RecvTimeoutError::Disconnected) => return Err(stopped()),
        }
        Ok(())
    }

    /// The wait before the next try, for tests.
    #[cfg(test)]
    fn backoff(&self) -> Duration {
        self.wait
    }
}

impl Read for Polled {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            self.check()?;
            match (&*self.socket).read(buf) {
                Ok(read) => {
                    self.wait = FIRST_WAIT;
                    return Ok(read);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => self.pause()?,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

/// Writes all of `bytes` to the non-blocking `socket`, waiting while its send buffer is full.
/// With a `stop`, the write fails once it fires.
pub(crate) fn write_all(
    socket: &TcpStream,
    mut bytes: &[u8],
    stop: Option<&Stop>,
) -> io::Result<()> {
    let mut wait = FIRST_WAIT;
    while !bytes.is_empty() {
        if stop.is_some_and(fired) {
            return Err(stopped());
        }
        match (&*socket).write(bytes) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(written) => {
                bytes = bytes.get(written..).unwrap_or_default();
                wait = FIRST_WAIT;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                wait_on(stop, wait)?;
                wait = wait.saturating_mul(2).min(IDLE_WAIT);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Whether `stop` has fired: its sender is gone. Only the handshake's writes pass a stop, and
/// nothing nudges that one.
fn fired(stop: &Stop) -> bool {
    matches!(stop.try_recv(), Err(TryRecvError::Disconnected))
}

/// Waits for `wait` before the next try; `Err` at once if `stop` fires first.
fn wait_on(stop: Option<&Stop>, wait: Duration) -> io::Result<()> {
    let Some(stop) = stop else {
        pause(wait);
        return Ok(());
    };
    match stop.recv_timeout(wait) {
        Ok(()) | Err(RecvTimeoutError::Timeout) => Ok(()),
        Err(RecvTimeoutError::Disconnected) => Err(stopped()),
    }
}

/// Waits for `after` (`std::thread::sleep` is banned in this workspace): the channel's sender is
/// held, so nothing ends the wait early.
fn pause(after: Duration) {
    let (_hold, wake) = mpsc::channel::<()>();
    if wake.recv_timeout(after).is_err() {
        // The time is up, as the sender is still held.
    }
}

/// The error a read or a write gives once its stop has fired: the link was ended, or the
/// handshake's deadline passed.
fn stopped() -> io::Error {
    io::Error::new(ErrorKind::ConnectionAborted, "the link was ended")
}

#[cfg(test)]
mod tests;

//! One connection: the handshake under its watchdog, then the pump. The pump reads a line only
//! while fewer than [`MAX_IN_FLIGHT`] messages are in flight, so a flood waits in the agent's
//! pipe, not in host memory; each message runs on its own thread and its answer is written
//! whole as it completes.

use std::io::{self, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, Condvar, Mutex};

use serde_json::Value;

#[cfg(doc)]
use super::super::limits::MAX_CONNECTIONS_PER_TOKEN;
use super::super::limits::{MAX_HELLO_BYTES, MAX_IN_FLIGHT, MAX_LINE_BYTES};
use super::super::texts;
use super::handshake::{self, Line, Refusal, Watchdog};
use super::{Session, hang_up, lock};

/// Starts connection `id`'s thread. The handshake's deadline is armed before this returns.
pub(super) fn start(session: &Arc<Session>, id: u64, stream: TcpStream) -> io::Result<()> {
    let watchdog = Watchdog::arm(session, stream.try_clone()?)?;
    let session = Arc::clone(session);
    std::thread::Builder::new()
        .name("clauseo-relay-conn".to_owned())
        .spawn(move || {
            let dispatch = session.dispatch.clone();
            tracing::dispatcher::with_default(&dispatch, || {
                run(&session, id, stream, watchdog);
                session.forget(id);
            });
        })
        .map(|_detached| ())
}

/// The handshake, then the pump until the agent or the session ends the connection.
fn run(session: &Arc<Session>, id: u64, stream: TcpStream, watchdog: Watchdog) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let line = handshake::read_line(&mut reader, MAX_HELLO_BYTES);
    let met = watchdog.meet();
    // Its thread ends now, and with the manual clock its timer is disarmed.
    drop(watchdog);
    if !met {
        session.log_refusal("handshake_timeout");
        return;
    }
    let verdict = match line {
        Line::Text(text) => handshake::check(session, &text),
        Line::TooLong => Err(Refusal::Silent("hello_too_long")),
        Line::End => Err(Refusal::Silent("hung_up")),
    };
    let flight = Arc::new(Flight::default());
    let verdict = verdict.and_then(|()| session.promote(id, &flight).map_err(Refusal::Answer));
    let writer = Writer(Mutex::new(stream));
    match verdict {
        Ok(()) => {}
        Err(Refusal::Silent(reason)) => {
            session.log_refusal(reason);
            writer.hang_up();
            return;
        }
        Err(Refusal::Answer(code)) => {
            session.log_refusal(code);
            if writer.line(&texts::hello_refused(code)).is_err() {
                // The peer is gone; it hears nothing either way.
            }
            writer.hang_up();
            return;
        }
    }
    if writer.line(&texts::hello_ok()).is_err() {
        writer.hang_up();
        return;
    }
    let conn = Arc::new(Conn {
        session: Arc::clone(session),
        writer,
        upstream: Mutex::new(Upstream::default()),
        flight,
    });
    pump(&conn, &mut reader);
}

/// What the connector told this connection, for the next requests' headers.
#[derive(Debug, Default)]
pub(crate) struct Upstream {
    /// `result.protocolVersion` of this connection's `initialize`, checked.
    pub(crate) protocol_version: Option<String>,
    /// The last `Mcp-Session-Id` the connector sent, checked.
    pub(crate) session_id: Option<String>,
}

/// A connection past its handshake.
pub(crate) struct Conn {
    pub(crate) session: Arc<Session>,
    pub(crate) writer: Writer,
    pub(crate) upstream: Mutex<Upstream>,
    pub(crate) flight: Arc<Flight>,
}

impl Conn {
    /// A connection past its handshake on `stream`, for a test that drives `forward` itself.
    #[cfg(test)]
    pub(crate) fn for_test(session: Arc<Session>, stream: TcpStream) -> Conn {
        Conn {
            session,
            writer: Writer(Mutex::new(stream)),
            upstream: Mutex::new(Upstream::default()),
            flight: Arc::new(Flight::default()),
        }
    }
}

/// The socket's write half: one line at a time, each whole.
pub(crate) struct Writer(Mutex<TcpStream>);

impl Writer {
    /// Writes `line` and a newline under the lock.
    pub(crate) fn line(&self, line: &[u8]) -> io::Result<()> {
        let mut stream = lock(&self.0);
        stream.write_all(line)?;
        stream.write_all(b"\n")?;
        stream.flush()
    }

    /// Shuts the connection down both ways.
    pub(crate) fn hang_up(&self) {
        hang_up(&lock(&self.0));
    }
}

/// The messages in flight on one connection, and whether it is closing.
#[derive(Debug, Default)]
pub(crate) struct Flight {
    state: Mutex<FlightState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct FlightState {
    in_flight: usize,
    closed: bool,
    /// Lines read so far, and times the pump waited for room (tests read both).
    lines: u64,
    stalls: u64,
}

impl Flight {
    /// Waits until a message may start; `false` once the connection is closing.
    fn wait_room(&self) -> bool {
        let mut state = lock(&self.state);
        if state.in_flight >= MAX_IN_FLIGHT && !state.closed {
            state.stalls = state.stalls.saturating_add(1);
            self.changed.notify_all();
        }
        while state.in_flight >= MAX_IN_FLIGHT && !state.closed {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        !state.closed
    }

    /// Waits until nothing is in flight, or the connection is closing.
    pub(crate) fn wait_idle(&self) {
        let mut state = lock(&self.state);
        while state.in_flight > 0 && !state.closed {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn begin(&self) {
        let mut state = lock(&self.state);
        state.in_flight = state.in_flight.saturating_add(1);
        state.lines = state.lines.saturating_add(1);
    }

    pub(crate) fn done(&self) {
        let mut state = lock(&self.state);
        state.in_flight = state.in_flight.saturating_sub(1);
        self.changed.notify_all();
    }

    /// The connection is closing: every wait ends.
    pub(crate) fn close(&self) {
        lock(&self.state).closed = true;
        self.changed.notify_all();
    }

    /// Waits until the pump has stalled `stalls` times, then gives the lines read (tests).
    #[cfg(test)]
    pub(crate) fn wait_stalls(&self, stalls: u64) -> u64 {
        let mut state = lock(&self.state);
        while state.stalls < stalls {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        state.lines
    }

    /// Lines read so far (tests).
    #[cfg(test)]
    pub(crate) fn lines(&self) -> u64 {
        lock(&self.state).lines
    }
}

/// Ends a message's flight, and frees its bytes, however its thread ends.
struct InFlight {
    conn: Arc<Conn>,
    bytes: usize,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.conn.session.free_bytes(self.bytes);
        self.conn.flight.done();
    }
}

/// Reads lines until the agent closes its side, the session closes the connection, or a line is
/// over [`MAX_LINE_BYTES`] (answered with the line-too-long error, then closed). However it ends,
/// the pump returns only once no message of this connection is in flight, so the connection's
/// slot is not freed while its calls still run: one token never has more than
/// [`MAX_CONNECTIONS_PER_TOKEN`] times [`MAX_IN_FLIGHT`] calls out. When the agent closes its
/// side, the answers in flight are written before the connection closes.
fn pump(conn: &Arc<Conn>, reader: &mut BufReader<TcpStream>) {
    loop {
        if !conn.flight.wait_room() {
            break;
        }
        match handshake::read_line(reader, MAX_LINE_BYTES) {
            Line::Text(text) if text.iter().all(u8::is_ascii_whitespace) => {}
            Line::Text(text) => {
                if !conn.session.wait_bytes(text.len()) {
                    break;
                }
                conn.flight.begin();
                if let Err(err) = spawn_message(conn, text) {
                    // The message's guard went with its closure and ended its flight.
                    tracing::warn!(error = %crate::log_file::Cause(&err), "case-law relay: no thread for a message");
                    conn.writer.hang_up();
                    break;
                }
            }
            Line::TooLong => {
                conn.session.log_refusal("line_too_long");
                let error = texts::rpc_error(&Value::Null, -32600, texts::LINE_TOO_LONG);
                if conn.writer.line(&error).is_err() {
                    // The peer is gone; the connection closes either way.
                }
                drain(reader);
                conn.writer.hang_up();
                break;
            }
            Line::End => break,
        }
    }
    conn.flight.wait_idle();
    conn.flight.close();
    conn.writer.hang_up();
}

/// Discards what the agent has already sent, without waiting for more, so the close after the
/// line-too-long error is a FIN the agent reads past, not a reset that loses the error. At most
/// [`MAX_LINE_BYTES`] more are read.
fn drain(reader: &mut BufReader<TcpStream>) {
    use std::io::Read as _;
    if reader.get_ref().set_nonblocking(true).is_err() {
        return;
    }
    let mut chunk = [0u8; 8192];
    let mut left = MAX_LINE_BYTES;
    while left > 0 {
        match reader.get_mut().read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => left = left.saturating_sub(read),
        }
    }
}

fn spawn_message(conn: &Arc<Conn>, line: Vec<u8>) -> io::Result<()> {
    let guard = InFlight {
        conn: Arc::clone(conn),
        bytes: line.len(),
    };
    std::thread::Builder::new()
        .name("clauseo-relay-call".to_owned())
        .spawn(move || {
            let conn = &guard.conn;
            let dispatch = conn.session.dispatch.clone();
            tracing::dispatcher::with_default(&dispatch, || super::forward::message(conn, &line));
            drop(guard);
        })
        .map(|_detached| ())
}

//! The version-history listener (`clauseo-versions`): the second loopback listener of every
//! launch, open whether or not case law is, with its own token. The agent's
//! `clauseo-mcp connector clauseo-versions` connects to it, and each JSON-RPC line it relays is
//! answered by the launch's [`VersionsBackend`], which reads only the launch's matter.
//!
//! What it shares with the case-law relay, one copy each (`gate.rs`): the loopback-only bind,
//! the token's digest and constant-time compare, the strict hello and its one answer to every
//! bad hello (`relay/handshake.rs` `greet`), and the accept loop with its backoff
//! (`relay/accept.rs`). Its own token means a case-law token, another launch's token or a
//! revoked one is `bad_token` here, as anywhere.
//!
//! Past the handshake: a line over [`MAX_VERSIONS_LINE_BYTES`] gets the line-too-long error and
//! the close; a line that is not JSON the parse error; an array the batch error; anything else
//! goes to [`VersionsBackend::answer`] on a thread of its own, at most [`MAX_IN_FLIGHT`] per
//! connection, and its answer, if any, is written whole. Once the token is revoked
//! ([`VersionsRelay::end`]) nothing more is answered. Limits shared with the relay:
//! [`MAX_PENDING_HANDSHAKES`], [`MAX_CONNECTIONS_PER_TOKEN`], [`MAX_IN_FLIGHT`] and the
//! handshake's deadline. Logs name the session and the reason, never a line or the token.
//!
//! Threads: one accept thread; one per connection; one per message in flight; one watchdog per
//! pending handshake. Locks: the gate before the connections.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_connectors::{Secret, new_relay_token};
use serde_json::Value;
use tracing::Dispatch;

use super::gate::{Phase, TokenGate, loopback_only};
#[cfg(doc)]
use super::limits::MAX_IN_FLIGHT;
use super::limits::{MAX_CONNECTIONS_PER_TOKEN, MAX_PENDING_HANDSHAKES};
use super::relay::accept::{self, Accepting};
use super::relay::conn::{Flight, Writer, drain};
use super::relay::handshake::{self, Door, Line, Watchdog};
use super::relay::{hang_up, lock};
use super::texts;
use crate::clock::Clock;
use crate::config::SessionKey;
use crate::versions::{SERVER_NAME, VersionsBackend, VersionsTurns};

/// The longest line the agent may send, without its newline. A call names a version and a path,
/// so a few hundred bytes; with [`MAX_IN_FLIGHT`] and [`MAX_CONNECTIONS_PER_TOKEN`] this bounds
/// what one token can make the host hold at 1 MiB of raw lines.
pub const MAX_VERSIONS_LINE_BYTES: usize = 64 * 1024;

/// The agent-facing texts of the listener's own errors (not interface words).
const LINE_TOO_LONG: &str = "Line too long for Clauseo's version-history relay.";
const NO_BATCHES: &str = "Clauseo's version-history relay does not accept JSON-RPC batches.";

/// One launch's version-history listener. Dropping it stops the listener: the token is
/// revoked, every connection is shut down, and nothing is accepted after.
pub(crate) struct VersionsRelay {
    inner: Arc<Listener>,
    addr: SocketAddr,
    accept: Option<JoinHandle<()>>,
}

/// Every listener opened and its address, so a host test can reach a session's listener (the
/// agent's entry holds the port, which the test cannot read).
#[cfg(feature = "test-support")]
static OPENED: Mutex<Vec<(SessionKey, SocketAddr)>> = Mutex::new(Vec::new());

/// Every version-history listener opened so far in this process and its address (host tests).
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub fn versions_opened_for_test() -> Vec<(SessionKey, SocketAddr)> {
    lock(&OPENED).clone()
}

struct Listener {
    key: SessionKey,
    backend: VersionsBackend,
    clock: Arc<dyn Clock>,
    gate: TokenGate,
    conns: Mutex<Conns>,
    /// The log dispatcher of the thread that opened the listener, for every thread it starts.
    dispatch: Dispatch,
}

/// The connections: those waiting on their handshake, and those past it.
#[derive(Default)]
struct Conns {
    pending: usize,
    open: usize,
    next: u64,
    live: BTreeMap<u64, Live>,
}

struct Live {
    /// A clone of the socket, so `end` and `stop` can shut it down.
    stream: TcpStream,
    /// Set once the handshake is through.
    flight: Option<Arc<Flight>>,
}

impl fmt::Debug for VersionsRelay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VersionsRelay")
            .field("session", &self.inner.key)
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl VersionsRelay {
    /// Starts `key`'s listener on `listener` (bound on loopback by the caller; any other
    /// address is `InvalidInput`), answering with `backend`, and mints its token. The token goes
    /// to the token file and the stderr redactor and nowhere else; the listener keeps its SHA-256.
    pub(crate) fn open(
        key: SessionKey,
        backend: VersionsBackend,
        clock: Arc<dyn Clock>,
        listener: TcpListener,
    ) -> io::Result<(VersionsRelay, Secret)> {
        let addr = listener.local_addr()?;
        loopback_only(
            addr,
            "the version-history listener listens on loopback only",
        )?;
        let token = new_relay_token();
        let inner = Arc::new(Listener {
            key,
            backend,
            clock,
            gate: TokenGate::new(&token),
            conns: Mutex::new(Conns::default()),
            dispatch: tracing::dispatcher::get_default(Dispatch::clone),
        });
        let accepting = Arc::clone(&inner);
        let accept = std::thread::Builder::new()
            .name("clauseo-versions".to_owned())
            .spawn(move || {
                let dispatch = accepting.dispatch.clone();
                tracing::dispatcher::with_default(&dispatch, || {
                    accept::accept_loop(&accepting, || listener.accept().map(|(stream, _)| stream));
                });
            })?;
        #[cfg(feature = "test-support")]
        lock(&OPENED).push((key, addr));
        Ok((
            VersionsRelay {
                inner,
                addr,
                accept: Some(accept),
            },
            token,
        ))
    }

    /// The listener's port, for `CLAUSEO_RELAY=127.0.0.1:<port>`.
    pub(crate) fn port(&self) -> u16 {
        self.addr.port()
    }

    /// The handle the host thread moves when a turn starts and ends: the backend's call caps.
    pub(crate) fn turns(&self) -> VersionsTurns {
        self.inner.backend.turns()
    }

    /// The session is ending: the token is revoked before this returns (so the token file can go
    /// next), every connection past its handshake is shut down, and a new handshake gets
    /// `bad_token` until the listener stops.
    pub(crate) fn end(&self) {
        self.inner.close(Phase::Ending);
    }
}

impl Drop for VersionsRelay {
    fn drop(&mut self) {
        self.inner.close(Phase::Stopped);
        // Wakes the accept loop, which sees `Stopped` and drops the listener.
        let woke = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1));
        if let (Ok(_wake), Some(accept)) = (woke, self.accept.take())
            && accept.join().is_err()
        {
            tracing::warn!("version-history listener: the accept thread panicked");
        }
    }
}

impl Listener {
    /// Moves to `to` (never back): revokes the token, then shuts down the connections past
    /// their handshake, and on `Stopped` the pending ones too.
    fn close(&self, to: Phase) {
        self.gate.close(to);
        let conns = lock(&self.conns);
        for live in conns.live.values() {
            if live.flight.is_some() || to == Phase::Stopped {
                hang_up(&live.stream);
            }
            if let Some(flight) = &live.flight {
                flight.close();
            }
        }
    }

    /// Moves connection `id` past its handshake; `busy` when this token already has its
    /// connections, `closed` when the listener is no longer open.
    fn promote(&self, id: u64, flight: &Arc<Flight>) -> Result<(), &'static str> {
        let gate = self.gate.while_open()?;
        let mut conns = lock(&self.conns);
        if conns.open >= MAX_CONNECTIONS_PER_TOKEN {
            return Err(texts::BUSY);
        }
        let Some(live) = conns.live.get_mut(&id) else {
            return Err(texts::CLOSED);
        };
        live.flight = Some(Arc::clone(flight));
        conns.pending = conns.pending.saturating_sub(1);
        conns.open = conns.open.saturating_add(1);
        drop(gate);
        Ok(())
    }
}

impl Door for Listener {
    fn gate(&self) -> &TokenGate {
        &self.gate
    }

    fn connector(&self) -> &str {
        SERVER_NAME
    }

    fn log_refusal(&self, reason: &str) {
        tracing::warn!(
            matter = %self.key.matter,
            session = %self.key.id(),
            reason,
            "version-history listener: connection refused"
        );
    }
}

impl Accepting for Listener {
    fn phase(&self) -> Phase {
        self.gate.phase()
    }

    fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }

    fn label(&self) -> &'static str {
        "version-history listener"
    }

    fn admit(&self, stream: &TcpStream) -> Option<u64> {
        let mut conns = lock(&self.conns);
        if conns.pending >= MAX_PENDING_HANDSHAKES {
            drop(conns);
            self.log_refusal("too_many_pending");
            return None;
        }
        let clone = stream.try_clone().ok()?;
        let id = conns.next;
        conns.next = conns.next.wrapping_add(1);
        conns.pending = conns.pending.saturating_add(1);
        conns.live.insert(
            id,
            Live {
                stream: clone,
                flight: None,
            },
        );
        Some(id)
    }

    fn start(self: &Arc<Self>, id: u64, stream: TcpStream) -> io::Result<()> {
        let watchdog = Watchdog::arm(self.clock.as_ref(), stream.try_clone()?)?;
        let listener = Arc::clone(self);
        std::thread::Builder::new()
            .name("clauseo-versions-conn".to_owned())
            .spawn(move || {
                let dispatch = listener.dispatch.clone();
                tracing::dispatcher::with_default(&dispatch, || {
                    run(&listener, id, stream, watchdog);
                    listener.forget(id);
                });
            })
            .map(|_detached| ())
    }

    fn forget(&self, id: u64) {
        let mut conns = lock(&self.conns);
        if let Some(live) = conns.live.remove(&id) {
            if live.flight.is_some() {
                conns.open = conns.open.saturating_sub(1);
            } else {
                conns.pending = conns.pending.saturating_sub(1);
            }
        }
    }
}

/// A connection past its handshake.
struct Conn {
    listener: Arc<Listener>,
    writer: Writer,
    flight: Arc<Flight>,
}

/// The handshake, then the pump until the agent or the session ends the connection.
fn run(listener: &Arc<Listener>, id: u64, stream: TcpStream, watchdog: Watchdog) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let flight = Arc::new(Flight::default());
    let writer = Writer::new(stream);
    let admit = || listener.promote(id, &flight);
    if !handshake::greet(listener.as_ref(), &mut reader, watchdog, &writer, admit) {
        return;
    }
    let conn = Arc::new(Conn {
        listener: Arc::clone(listener),
        writer,
        flight,
    });
    pump(&conn, &mut reader);
}

/// Reads lines until the agent closes its side, the session closes the connection, or a line is
/// over [`MAX_VERSIONS_LINE_BYTES`]. It returns only once no message of this connection is in
/// flight, so a token never has more than [`MAX_CONNECTIONS_PER_TOKEN`] times [`MAX_IN_FLIGHT`]
/// calls out.
fn pump(conn: &Arc<Conn>, reader: &mut BufReader<TcpStream>) {
    loop {
        if !conn.flight.wait_room() {
            break;
        }
        match handshake::read_line(reader, MAX_VERSIONS_LINE_BYTES) {
            Line::Text(text) if text.iter().all(u8::is_ascii_whitespace) => {}
            Line::Text(text) => {
                conn.flight.begin();
                if let Err(err) = spawn_message(conn, text) {
                    tracing::warn!(error = %crate::log_file::Cause(&err), "version-history listener: no thread for a message");
                    conn.writer.hang_up();
                    break;
                }
            }
            Line::TooLong => {
                conn.listener.log_refusal("line_too_long");
                let error = texts::rpc_error(&Value::Null, -32600, LINE_TOO_LONG);
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

/// Ends a message's flight however its thread ends.
struct InFlight(Arc<Conn>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.flight.done();
    }
}

fn spawn_message(conn: &Arc<Conn>, line: Vec<u8>) -> io::Result<()> {
    let guard = InFlight(Arc::clone(conn));
    std::thread::Builder::new()
        .name("clauseo-versions-call".to_owned())
        .spawn(move || {
            let conn = &guard.0;
            let dispatch = conn.listener.dispatch.clone();
            tracing::dispatcher::with_default(&dispatch, || message(conn, &line));
            drop(guard);
        })
        .map(|_detached| ())
}

/// Handles one line from the agent: the framing here, the rest in the backend.
fn message(conn: &Conn, line: &[u8]) {
    let value = match serde_json::from_slice::<Value>(line) {
        Ok(Value::Array(_)) => {
            answer(conn, &texts::rpc_error(&Value::Null, -32600, NO_BATCHES));
            return;
        }
        Ok(value) => value,
        Err(_not_json) => {
            let error = texts::rpc_error(&Value::Null, -32700, texts::PARSE_ERROR);
            answer(conn, &error);
            return;
        }
    };
    if conn.listener.gate.phase() != Phase::Open {
        // The session is ending; its connections are closing.
        return;
    }
    if let Some(reply) = conn.listener.backend.answer(&value) {
        answer(conn, &texts::line(&reply));
    }
}

/// Writes one line; a write that fails means the agent is gone, and its reader ends.
fn answer(conn: &Conn, line: &[u8]) {
    if conn.writer.line(line).is_err() {
        conn.writer.hang_up();
    }
}

#[cfg(test)]
mod tests;

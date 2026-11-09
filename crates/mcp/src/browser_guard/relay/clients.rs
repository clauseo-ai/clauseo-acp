//! Unix only: listen mode's clients (`browser-guard --listen`), one at a time, each served as the
//! agent's stdio is: its lines through the filter and [`pumps::pump_lines`], and every line to the
//! agent written to it ([`Out::attach`](super::lines::Out::attach)).
//!
//! It sees the transport only through `crate::transport`'s [`Listener`] and [`Connection`]. A
//! thread accepts each connection. One whose peer runs as another user ([`Connection::same_user`])
//! is closed at once. One that arrives while a client is connected gets a single JSON-RPC error
//! line, [`BUSY`], and is closed: with the id of its first request, if one arrives within
//! [`BUSY_WAIT`] (the bridge sends the request that made it connect at once), so that request is
//! answered once; with a null id otherwise. Otherwise the connection becomes the client, on a
//! thread of its own.
//!
//! Each write to the client may block for at most [`WRITE_WAIT`]. A client that stops reading
//! for that long, or whose connection fails, is taken as gone: the guard shuts the connection,
//! which ends the client's reads and so counts as its disconnect, and every later write to it
//! fails at once. So no write to a jailed agent that stopped reading holds the agent's output
//! lock for ever, and the guard's end (which answers what is in flight) is never stuck on one.
//!
//! When the client disconnects the guard stays up, for the next client: it closes the
//! connection, sends the agent's lines nowhere until the next one, and stops the browser exactly
//! as the idle stop does (`session::stop`: agent-browser, its daemon and Chrome end, the run's
//! profile stays). Requests still in flight go unanswered: no one is left to read the answer, and
//! the bridge answers what it sent itself. The next client's `initialize` goes to a one-shot
//! agent-browser and its first tool call starts the browser again, as at the guard's start.
//! Lines from a start the disconnect stopped never reach the next client (the floor in
//! [`Out`](super::lines::Out)).
//!
//! A client's disconnect never ends the guard. What ends it is what ends it over stdio, but for
//! the agent's leaving: the parent's going, a start that fails, agent-browser's own end, Chrome's
//! second death or a refused port (`relay.rs`).

use std::io::{self, BufReader, Write};
use std::net::Shutdown;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use super::super::parent::Parent;
use super::super::session::{self, Shared};
use super::lines::{Line, Overflow, error_reply, lock, read_line};
use super::pumps::{self, Pumped};
use super::{Event, oneshot};
use crate::browser_guard::filter::MAX_LINE_BYTES;
use crate::browser_guard::{note, pause};
use crate::connector::head;
use crate::transport::{Connection, Listener};

/// The error a connection gets while another client is connected.
pub(crate) const BUSY: &str =
    "The browser is in use by another connection. Close that one first, then connect again.";
/// How long the guard waits for a second connection's first request, and then tries to write
/// [`BUSY`], before closing it anyway.
const BUSY_WAIT: Duration = Duration::from_secs(1);
/// The longest one write to the client may block before the guard takes the client as gone.
pub(crate) const WRITE_WAIT: Duration = Duration::from_secs(10);
/// Tests only (the `test-support` feature, never in a shipped build): [`WRITE_WAIT`] in
/// milliseconds, read from the guard's own environment.
#[cfg(feature = "test-support")]
const WRITE_WAIT_FOR_TESTS: &str = "CLAUSEO_TEST_GUARD_WRITE_MS";
/// The pause after an `accept` that failed (out of descriptors, say), before the next.
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// Starts the thread that accepts clients on `listener`. Never joined: it is blocked in `accept`
/// when the guard exits.
pub(crate) fn serve(
    listener: impl Listener,
    shared: &Arc<Shared>,
    parent: &Parent,
    events: mpsc::Sender<Event>,
) {
    let (shared, parent) = (Arc::clone(shared), parent.clone());
    drop(thread::spawn(move || {
        accept(&listener, &shared, &parent, &events);
    }));
}

/// Accepts connections for as long as the guard runs, making each the client or closing it.
fn accept(
    listener: &impl Listener,
    shared: &Arc<Shared>,
    parent: &Parent,
    events: &mpsc::Sender<Event>,
) -> ! {
    let connected = Arc::new(AtomicBool::new(false));
    loop {
        let Ok(stream) = listener.accept() else {
            pause(ACCEPT_RETRY);
            continue;
        };
        if !stream.same_user() {
            note("closed a connection from another user");
            continue;
        }
        // Only this thread sets it, and only the client's thread clears it.
        if connected.load(Ordering::SeqCst) {
            refuse(stream);
            continue;
        }
        connected.store(true, Ordering::SeqCst);
        let (shared, parent, events, connected) = (
            Arc::clone(shared),
            parent.clone(),
            events.clone(),
            Arc::clone(&connected),
        );
        drop(thread::spawn(move || {
            if client(&stream, &shared, &parent, &events) {
                connected.store(false, Ordering::SeqCst);
            }
        }));
    }
}

/// [`WRITE_WAIT`], or in a `test-support` build the test's.
fn write_wait() -> Duration {
    #[cfg(feature = "test-support")]
    if let Some(ms) = std::env::var(WRITE_WAIT_FOR_TESTS)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Duration::from_millis(ms);
    }
    WRITE_WAIT
}

/// The client's side of [`Out`](super::lines::Out): its connection, shut at the first write
/// that fails or times out, so its reads end (its disconnect) and later writes fail at once.
struct ToClient<C: Connection>(C);

impl<C: Connection> ToClient<C> {
    /// Shuts the connection when `result` is a failure other than an interrupted call.
    fn checked<T>(&self, result: io::Result<T>) -> io::Result<T> {
        if let Err(error) = &result
            && error.kind() != io::ErrorKind::Interrupted
        {
            drop(self.0.shutdown(Shutdown::Both));
        }
        result
    }
}

impl<C: Connection> Write for ToClient<C> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.0.write(buf);
        self.checked(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        let flushed = self.0.flush();
        self.checked(flushed)
    }
}

/// Tells a second connection the browser is in use, on a thread of its own (it waits up to
/// [`BUSY_WAIT`] for the connection's first request), then closes it.
fn refuse(mut stream: impl Connection) {
    note("refused a second connection while a client is connected");
    drop(thread::spawn(move || {
        let id = if stream.set_read_timeout(Some(BUSY_WAIT)).is_ok()
            && let Ok(reader) = stream.try_clone()
        {
            first_request_id(reader)
        } else {
            None
        };
        let line = format!(
            "{}\n",
            error_reply(&id.unwrap_or(serde_json::Value::Null), BUSY)
        );
        // A peer that does not read, or has gone, gets the close alone.
        if stream.set_write_timeout(Some(BUSY_WAIT)).is_ok() {
            drop(stream.write_all(line.as_bytes()));
        }
    }));
}

/// The id of the first line read from `reader`, if it is a request.
fn first_request_id(reader: impl Connection) -> Option<serde_json::Value> {
    let mut line = Vec::new();
    let read = read_line(
        &mut BufReader::new(reader),
        &mut line,
        MAX_LINE_BYTES,
        Overflow::Stop,
    );
    if !matches!(read, Ok(Line::Whole | Line::Partial)) {
        return None;
    }
    head::read(&line).and_then(|head| if head.method { head.id } else { None })
}

/// Serves one client until it disconnects or the guard is ending. `true` once it has
/// disconnected and the browser is stopped, so the next client may connect; `false` when the
/// guard is ending, which takes no more clients.
fn client(
    stream: &impl Connection,
    shared: &Arc<Shared>,
    parent: &Parent,
    events: &mpsc::Sender<Event>,
) -> bool {
    let (Ok(writer), Ok(reader)) = (stream.try_clone(), stream.try_clone()) else {
        note("cannot serve a client: its connection cannot be shared between threads");
        return true;
    };
    if writer.set_write_timeout(Some(write_wait())).is_err() {
        note("cannot serve a client: its writes cannot be bounded");
        return true;
    }
    let floor = lock(&shared.state).generation;
    shared.out.attach(Box::new(ToClient(writer)), floor);
    note("a client connected");
    let mut input = BufReader::new(reader);
    if pumps::pump_lines(&mut input, shared, parent, events) == Pumped::Ending {
        return false;
    }
    // Wakes a write blocked on a client that stopped reading, then lets go of the connection.
    drop(stream.shutdown(Shutdown::Both));
    shared.out.detach();
    let stopped = {
        let mut state = lock(&shared.state);
        // The relay's end stops the browser itself.
        if state.ended {
            return false;
        }
        let stopped = session::stop(&shared.launcher, &mut state);
        drop(state.take_in_flight());
        stopped.is_some()
    };
    // None runs: the client's own thread runs each one to its end. Kept as the relay's end does.
    drop(oneshot::end(shared));
    note(if stopped {
        "the client disconnected; stopped Chrome and agent-browser, keeping the profile until the \
         session ends"
    } else {
        "the client disconnected"
    });
    true
}

#[cfg(test)]
mod tests;

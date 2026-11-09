//! One connection to the guard. The stdin side writes lines into it ([`Link::send`]); a thread of
//! its own copies the guard's lines to stdout, whole and unchanged, until the connection ends.
//!
//! A request is pending from just before it is written until a reply with its id comes back.
//! When the connection ends, the reader closes the pending set and answers every request left in
//! it with the bridge's error; a request the stdin side adds after that is refused, and answered
//! by the stdin side instead. Whichever side takes a request off the set answers it, so each is
//! answered once.

use std::io::{self, BufReader};
use std::net::Shutdown;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::{Out, unavailable, write_line};
use crate::browser_guard::lines::{Line, Overflow, read_line};
use crate::connector::head::{self, Head};
use crate::transport::Connection;

/// The requests sent and not yet answered; `None` once the connection has ended.
#[derive(Default)]
struct Pending {
    ids: Mutex<Option<Vec<Value>>>,
}

impl Pending {
    fn open() -> Pending {
        Pending {
            ids: Mutex::new(Some(Vec::new())),
        }
    }

    /// Notes request `id`; `false` once the connection has ended.
    fn add(&self, id: &Value) -> bool {
        self.lock()
            .as_mut()
            .map(|ids| ids.push(id.clone()))
            .is_some()
    }

    /// Takes request `id` off; `false` when it is not pending (answered, or never sent).
    fn release(&self, id: &Value) -> bool {
        let mut ids = self.lock();
        let Some(at) = ids
            .as_ref()
            .and_then(|ids| ids.iter().position(|sent| sent == id))
        else {
            return false;
        };
        ids.as_mut().map(|ids| ids.remove(at)).is_some()
    }

    /// Ends the set and returns every request left in it.
    fn close(&self) -> Vec<Value> {
        self.lock().take().unwrap_or_default()
    }

    fn ended(&self) -> bool {
        self.lock().is_none()
    }

    fn lock(&self) -> MutexGuard<'_, Option<Vec<Value>>> {
        // A panicked holder left the list whole; carry on.
        self.ids.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What [`Link::send`] did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Sent {
    /// The line went out.
    Sent,
    /// The connection has ended; drop it. `answer`: the request, if the line was one, is the
    /// stdin side's to answer.
    Lost { answer: bool },
}

/// A connection to the guard and its reader thread.
pub(crate) struct Link<C: Connection> {
    socket: C,
    pending: Arc<Pending>,
    /// Disconnected once the reader thread has ended.
    done: Receiver<()>,
}

impl<C: Connection> Link<C> {
    /// Starts the reader thread on `socket`, copying the guard's lines (each at most `cap`
    /// bytes) to `out`.
    pub(crate) fn start(socket: C, out: &Out, cap: usize) -> io::Result<Link<C>> {
        let reader = socket.try_clone()?;
        let pending = Arc::new(Pending::open());
        let (done_tx, done) = mpsc::channel::<()>();
        let (out, shared) = (Arc::clone(out), Arc::clone(&pending));
        drop(thread::spawn(move || {
            relay_to_agent(reader, &out, &shared, cap);
            drop(done_tx);
        }));
        Ok(Link {
            socket,
            pending,
            done,
        })
    }

    /// Whether the connection has ended (the reader has closed the pending set).
    pub(crate) fn ended(&self) -> bool {
        self.pending.ended()
    }

    /// Writes `line` and a newline; a request (`id`) is pending first.
    pub(crate) fn send(&mut self, line: &[u8], id: Option<&Value>) -> Sent {
        if let Some(id) = id
            && !self.pending.add(id)
        {
            return Sent::Lost { answer: true };
        }
        let mut whole = Vec::with_capacity(line.len() + 1);
        whole.extend_from_slice(line);
        whole.push(b'\n');
        if self.socket.write_all(&whole).is_ok() {
            return Sent::Sent;
        }
        drop(self.socket.shutdown(Shutdown::Both));
        Sent::Lost {
            answer: id.is_some_and(|id| self.pending.release(id)),
        }
    }

    /// Stdin has ended: tells the guard (a half-close), waits up to `grace` for its last lines
    /// and its close, then ends the connection. Returns the requests still unanswered, which the
    /// stdin side answers; the reader answers none of them any more.
    pub(crate) fn finish(self, grace: Duration) -> Vec<Value> {
        drop(self.socket.shutdown(Shutdown::Write));
        if self.done.recv_timeout(grace).is_err() {
            // The reader ended (its sender is gone), or the grace is over: go on either way.
        }
        drop(self.socket.shutdown(Shutdown::Both));
        self.pending.close()
    }
}

/// Copies the guard's lines to `out` until the connection ends, a line is over `cap` or cut off,
/// or `out` fails; a reply releases its request. Then shuts the connection, so the stdin side's
/// next write fails, and answers every request left.
fn relay_to_agent(socket: impl Connection, out: &Out, pending: &Pending, cap: usize) {
    let mut reader = BufReader::new(socket);
    let mut line = Vec::new();
    while let Ok(Line::Whole) = read_line(&mut reader, &mut line, cap, Overflow::Stop) {
        if let Some(Head {
            id: Some(id),
            method: false,
        }) = head::read(&line)
        {
            pending.release(&id);
        }
        if write_line(out, &line).is_err() {
            break;
        }
    }
    drop(reader.get_ref().shutdown(Shutdown::Both));
    for id in pending.close() {
        if write_line(out, &unavailable(&id)).is_err() {
            break;
        }
    }
}

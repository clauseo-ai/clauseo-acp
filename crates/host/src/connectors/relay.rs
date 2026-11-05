//! [`Relays`] and [`RelaySession`]: the listener, its accept loop, the connection registry, the
//! token and the session's lifetime. The handshake is in `handshake.rs`, the pump of one
//! connection in `conn.rs`, and what happens to one message in `forward.rs`.
//!
//! Threads: one accept thread per listener; one thread per connection, which reads the hello
//! and then the lines; one per message in flight; one watchdog per pending handshake. Locks: the
//! gate before the connections; the turn caps before the day count; the day writer's before the
//! day count. Two are held across I/O: the line writer's, which makes each line whole, and the
//! day writer's across the `day.json` write and its fsyncs. The day count's lock is not: a paid
//! call is counted in memory under it, then waits (lock released) until `day.json` holds that
//! count or a newer one, before it goes out.

mod accept;
mod conn;
mod forward;
mod handshake;

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use clauseo_connectors::{ConnectorHandle, Origin, Secret, new_relay_token};
use sha2::{Digest as _, Sha256};
use tracing::Dispatch;

use super::RelayEvents;
use super::limits::{
    DayCount, DayWriter, LocalDay, MAX_IN_FLIGHT_BYTES_PER_SESSION, MAX_PENDING_HANDSHAKES,
    TurnCaps,
};
use crate::clock::Clock;
use crate::config::SessionKey;

use conn::Flight;

/// What every listener of the app shares.
pub struct RelayConfig {
    /// The connector the relay forwards to (`case-law`).
    pub connector: ConnectorHandle,
    /// The origin the connector's config uses, for its pay links (`Origin::Production` in the
    /// app).
    pub origin: Origin,
    /// `<data_dir>/connectors/day.json`.
    pub day_file: PathBuf,
    /// The host's clock: the handshake deadline, the local day and the overhead timing.
    pub clock: Arc<dyn Clock>,
    /// The OS's time zone, for the day cap.
    pub local_day: Arc<dyn LocalDay>,
    /// Where the relay's events go.
    pub events: Arc<dyn RelayEvents>,
}

/// The app's relays: build once, then [`Relays::open`] one session per agent launch.
#[derive(Clone)]
pub struct Relays {
    hub: Arc<Hub>,
}

pub(crate) struct Hub {
    pub(crate) connector: ConnectorHandle,
    pub(crate) origin: Origin,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) local_day: Arc<dyn LocalDay>,
    pub(crate) events: Arc<dyn RelayEvents>,
    pub(crate) day: Mutex<DayCount>,
    /// Every session opened and its address, so a host test can reach a session's relay.
    #[cfg(feature = "test-support")]
    opened: Mutex<Vec<(SessionKey, SocketAddr)>>,
    pub(crate) day_writer: DayWriter,
}

impl fmt::Debug for Relays {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Relays")
            .field("connector", &self.hub.connector.known().id)
            .finish_non_exhaustive()
    }
}

impl Relays {
    /// Reads `day.json` and holds the rest. Binds nothing.
    pub fn new(config: RelayConfig) -> Relays {
        let day = DayCount::load(&config.day_file);
        Relays {
            hub: Arc::new(Hub {
                connector: config.connector,
                origin: config.origin,
                clock: config.clock,
                local_day: config.local_day,
                events: config.events,
                day: Mutex::new(day),
                day_writer: DayWriter::new(config.day_file),
                #[cfg(feature = "test-support")]
                opened: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Every session opened so far and its relay's address (host tests: the agent's entry holds
    /// the port, which the test cannot read).
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn opened_for_test(&self) -> Vec<(SessionKey, SocketAddr)> {
        lock(&self.hub.opened).clone()
    }

    /// The connector the relays forward to: the launch prep asks it whether it is signed in.
    pub(crate) fn connector(&self) -> &ConnectorHandle {
        &self.hub.connector
    }

    /// Starts `session`'s relay on `listener` (bound on loopback by the caller; any other address
    /// is `InvalidInput`) and mints its token. The token goes to the token file and nowhere else;
    /// the relay keeps its SHA-256.
    pub fn open(
        &self,
        session: SessionKey,
        listener: TcpListener,
    ) -> io::Result<(RelaySession, Secret)> {
        let addr = listener.local_addr()?;
        if !addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the case-law relay listens on loopback only",
            ));
        }
        let token = new_relay_token();
        let inner = Arc::new(Session {
            hub: Arc::clone(&self.hub),
            key: session,
            gate: Mutex::new(Gate {
                phase: Phase::Open,
                digest: Some(digest(token.expose().as_bytes())),
            }),
            caps: Mutex::new(TurnCaps::default()),
            conns: Mutex::new(Conns::default()),
            bytes: Mutex::new(Bytes::default()),
            bytes_freed: Condvar::new(),
            dispatch: tracing::dispatcher::get_default(Dispatch::clone),
        });
        #[cfg(feature = "test-support")]
        lock(&self.hub.opened).push((session, addr));
        let accepting = Arc::clone(&inner);
        let accept = std::thread::Builder::new()
            .name("clauseo-relay".to_owned())
            .spawn(move || {
                let dispatch = accepting.dispatch.clone();
                tracing::dispatcher::with_default(&dispatch, || {
                    accept::accept_loop(&accepting, || listener.accept().map(|(stream, _)| stream));
                });
            })?;
        Ok((
            RelaySession {
                inner,
                addr,
                accept: Some(accept),
            },
            token,
        ))
    }
}

/// Where a session is in its life. Only forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Phase {
    Open,
    /// The token is revoked: handshakes get `closed`, and nothing more is forwarded.
    Ending,
    /// The listener is closing.
    Stopped,
}

/// The token's digest and the phase, under one lock: a handshake sees both or neither change.
struct Gate {
    phase: Phase,
    /// `None` once the token is revoked.
    digest: Option<[u8; 32]>,
}

/// The connections: those waiting on their handshake, and those past it.
#[derive(Default)]
pub(crate) struct Conns {
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

/// The session's in-flight line bytes.
#[derive(Debug, Default)]
pub(crate) struct Bytes {
    used: usize,
    /// First come, first served: the next ticket to hand out, and the one whose turn it is.
    next_ticket: u64,
    serving: u64,
    /// How many times a pump waited for room (tests read it).
    waits: u64,
}

/// One launch's relay, shared by its threads.
pub(crate) struct Session {
    pub(crate) hub: Arc<Hub>,
    pub(crate) key: SessionKey,
    gate: Mutex<Gate>,
    pub(crate) caps: Mutex<TurnCaps>,
    conns: Mutex<Conns>,
    /// The raw bytes of the lines in flight on all its connections, and the waits for room.
    bytes: Mutex<Bytes>,
    bytes_freed: Condvar,
    /// The log dispatcher of the thread that opened the relay, for every thread it starts.
    pub(crate) dispatch: Dispatch,
}

/// A poisoned lock is still usable: each guarded value is whole between statements.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Compares two digests in constant time.
fn same_digest(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let diff = left
        .iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b));
    std::hint::black_box(diff) == 0
}

impl Session {
    pub(crate) fn phase(&self) -> Phase {
        lock(&self.gate).phase
    }

    /// Whether `token` is this listener's live token, in constant time. `Err(())` once the
    /// token is revoked: the session is ending.
    pub(crate) fn token_matches(&self, token: &str) -> Result<bool, ()> {
        let presented = digest(token.as_bytes());
        let gate = lock(&self.gate);
        match (gate.phase, gate.digest.as_ref()) {
            (Phase::Open, Some(ours)) => Ok(same_digest(ours, &presented)),
            _ => Err(()),
        }
    }

    /// Takes `len` bytes of the session's in-flight budget,
    /// [`MAX_IN_FLIGHT_BYTES_PER_SESSION`], waiting while others hold it; a line alone may take
    /// it whole. Lines are let in in the order they asked, so a large line waiting for room is
    /// not passed by small ones. `false` once the session is no longer open.
    pub(crate) fn wait_bytes(&self, len: usize) -> bool {
        let mut bytes = lock(&self.bytes);
        let ticket = bytes.next_ticket;
        bytes.next_ticket = bytes.next_ticket.wrapping_add(1);
        let mut waited = false;
        loop {
            if self.phase() != Phase::Open {
                // Pass the turn on; every later ticket sees the phase too.
                bytes.serving = bytes.serving.wrapping_add(1);
                self.bytes_freed.notify_all();
                return false;
            }
            let fits = bytes.used == 0
                || bytes.used.saturating_add(len) <= MAX_IN_FLIGHT_BYTES_PER_SESSION;
            if fits && bytes.serving == ticket {
                bytes.used = bytes.used.saturating_add(len);
                bytes.serving = bytes.serving.wrapping_add(1);
                self.bytes_freed.notify_all();
                return true;
            }
            if !waited {
                waited = true;
                bytes.waits = bytes.waits.saturating_add(1);
                self.bytes_freed.notify_all();
            }
            bytes = self
                .bytes_freed
                .wait(bytes)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Gives back `len` bytes a message held.
    pub(crate) fn free_bytes(&self, len: usize) {
        let mut bytes = lock(&self.bytes);
        bytes.used = bytes.used.saturating_sub(len);
        self.bytes_freed.notify_all();
    }

    /// Waits until pumps have waited for byte room `waits` times (tests).
    #[cfg(test)]
    pub(crate) fn wait_byte_waits(&self, waits: u64) {
        let mut bytes = lock(&self.bytes);
        while bytes.waits < waits {
            bytes = self
                .bytes_freed
                .wait(bytes)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Moves to `to` (never back): revokes the token, then shuts down the connections past
    /// their handshake, and on `Stopped` the pending ones too.
    fn close(&self, to: Phase) {
        {
            let mut gate = lock(&self.gate);
            gate.phase = gate.phase.max(to);
            gate.digest = None;
        }
        let conns = lock(&self.conns);
        for live in conns.live.values() {
            if live.flight.is_some() || to == Phase::Stopped {
                hang_up(&live.stream);
            }
            if let Some(flight) = &live.flight {
                flight.close();
            }
        }
        drop(conns);
        // A pump waiting for byte room sees the phase and stops.
        let _bytes = lock(&self.bytes);
        self.bytes_freed.notify_all();
    }

    /// Registers a new connection waiting on its handshake; `None` when the listener already
    /// holds [`MAX_PENDING_HANDSHAKES`] or is stopping, and the socket is dropped at once.
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

    /// Moves connection `id` past its handshake, with its flight; `false` when this token
    /// already has its connections (`busy`) or the session is no longer open.
    pub(crate) fn promote(&self, id: u64, flight: &Arc<Flight>) -> Result<(), &'static str> {
        let gate = lock(&self.gate);
        if gate.phase != Phase::Open {
            return Err(super::texts::CLOSED);
        }
        let mut conns = lock(&self.conns);
        if conns.open >= super::limits::MAX_CONNECTIONS_PER_TOKEN {
            return Err(super::texts::BUSY);
        }
        let Some(live) = conns.live.get_mut(&id) else {
            return Err(super::texts::CLOSED);
        };
        live.flight = Some(Arc::clone(flight));
        conns.pending = conns.pending.saturating_sub(1);
        conns.open = conns.open.saturating_add(1);
        drop(gate);
        Ok(())
    }

    /// Forgets connection `id`, whichever stage it reached.
    pub(crate) fn forget(&self, id: u64) {
        let mut conns = lock(&self.conns);
        if let Some(live) = conns.live.remove(&id) {
            if live.flight.is_some() {
                conns.open = conns.open.saturating_sub(1);
            } else {
                conns.pending = conns.pending.saturating_sub(1);
            }
        }
    }

    /// Logs a refused or dropped connection: the session and the reason, nothing it sent.
    pub(crate) fn log_refusal(&self, reason: &str) {
        tracing::warn!(
            matter = %self.key.matter,
            session = %self.key.id(),
            reason,
            "case-law relay: connection refused"
        );
    }

    /// The flight of the first connection past its handshake (tests).
    #[cfg(test)]
    pub(crate) fn first_flight(&self) -> Option<Arc<Flight>> {
        lock(&self.conns)
            .live
            .values()
            .find_map(|live| live.flight.clone())
    }
}

/// Shuts both halves of `stream`; its reader ends at once.
pub(crate) fn hang_up(stream: &TcpStream) {
    if stream.shutdown(Shutdown::Both).is_err() {
        // Already gone: the peer or another thread closed it first.
    }
}

/// One launch's relay. Dropping it stops the listener: the token is revoked, every connection
/// is shut down, and nothing is accepted after.
pub struct RelaySession {
    inner: Arc<Session>,
    addr: SocketAddr,
    accept: Option<JoinHandle<()>>,
}

impl fmt::Debug for RelaySession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelaySession")
            .field("session", &self.inner.key)
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl RelaySession {
    /// The listener's address, `127.0.0.1:<port>`, for `CLAUSEO_RELAY`.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The handle the host thread moves when a turn starts and ends.
    pub fn turns(&self) -> TurnHandle {
        TurnHandle(Arc::clone(&self.inner))
    }

    /// The session is ending: the token is revoked before this returns (so the token file can
    /// go next), every connection past its handshake is shut down, and a new handshake gets
    /// `closed` until the listener stops.
    pub fn end(&self) {
        self.inner.close(Phase::Ending);
    }

    /// Stops the listener now; the same as dropping it.
    pub fn stop(self) {
        drop(self);
    }

    /// The session's shared state (tests).
    #[cfg(test)]
    pub(crate) fn shared(&self) -> &Arc<Session> {
        &self.inner
    }
}

impl Drop for RelaySession {
    fn drop(&mut self) {
        self.inner.close(Phase::Stopped);
        // Wakes the accept loop, which sees `Stopped` and drops the listener. Without the wake
        // (no connection possible), the thread is left to end at its next accept.
        let woke = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1));
        if let (Ok(_wake), Some(accept)) = (woke, self.accept.take())
            && accept.join().is_err()
        {
            tracing::warn!("case-law relay: the accept thread panicked");
        }
    }
}

/// Where the session's turn is: written by the host thread, read when a call is counted.
#[derive(Clone)]
pub struct TurnHandle(Arc<Session>);

impl fmt::Debug for TurnHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TurnHandle").field(&self.0.key).finish()
    }
}

impl TurnHandle {
    /// A turn starts: its counts, and the between-turns bucket's, start at 0.
    pub fn start(&self) {
        lock(&self.0.caps).start_turn();
    }

    /// The turn ended: calls now count against the between-turns bucket.
    pub fn end(&self) {
        lock(&self.0.caps).end_turn();
    }
}

#[cfg(test)]
mod tests;

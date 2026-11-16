//! The accept loop. A failed accept that lasts (out of file descriptors, say) is retried after
//! a pause that doubles from [`BACKOFF_FIRST`] to [`BACKOFF_MOST`] on the host's clock, and is
//! logged once per streak, with the streak's length when it ends, so it neither spins a core nor
//! floods the log. Both listeners of a launch run it, each through its [`Accepting`]: the
//! case-law relay's `Session` and the version-history listener (`versions_relay.rs`).

use std::io;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use super::{Phase, Session, conn};
use crate::clock::Clock;

/// What the accept loop needs of a listener.
pub(crate) trait Accepting: Send + Sync {
    /// The listener's phase: the loop ends once it is `Stopped`.
    fn phase(&self) -> Phase;
    /// The host's clock, for the pauses.
    fn clock(&self) -> &dyn Clock;
    /// The listener as the log names it (`case-law relay`).
    fn label(&self) -> &'static str;
    /// Registers a new connection waiting on its handshake; `None` drops it.
    fn admit(&self, stream: &TcpStream) -> Option<u64>;
    /// Starts connection `id`'s thread.
    fn start(self: &Arc<Self>, id: u64, stream: TcpStream) -> io::Result<()>;
    /// Forgets connection `id`.
    fn forget(&self, id: u64);
}

impl Accepting for Session {
    fn phase(&self) -> Phase {
        Session::phase(self)
    }

    fn clock(&self) -> &dyn Clock {
        self.hub.clock.as_ref()
    }

    fn label(&self) -> &'static str {
        "case-law relay"
    }

    fn admit(&self, stream: &TcpStream) -> Option<u64> {
        Session::admit(self, stream)
    }

    fn start(self: &Arc<Self>, id: u64, stream: TcpStream) -> io::Result<()> {
        conn::start(self, id, stream)
    }

    fn forget(&self, id: u64) {
        Session::forget(self, id);
    }
}

/// The pause after the first failed accept of a streak.
pub(super) const BACKOFF_FIRST: Duration = Duration::from_millis(10);
/// The longest pause.
pub(super) const BACKOFF_MOST: Duration = Duration::from_secs(1);

/// Accepts with `accept` until the session is stopped (the stop wakes it with a connection).
pub(crate) fn accept_loop<S: Accepting + ?Sized>(
    session: &Arc<S>,
    mut accept: impl FnMut() -> io::Result<TcpStream>,
) {
    let listener = session.label();
    let mut failures: u64 = 0;
    loop {
        let accepted = accept();
        if session.phase() == Phase::Stopped {
            break;
        }
        let stream = match accepted {
            Ok(stream) => stream,
            Err(err) => {
                if failures == 0 {
                    tracing::warn!(listener, error = %crate::log_file::Cause(&err), "accept failed; retrying with a pause");
                }
                let pause = backoff(failures);
                failures = failures.saturating_add(1);
                futures::executor::block_on(session.clock().after(pause));
                continue;
            }
        };
        if failures > 0 {
            tracing::warn!(listener, failures, "accept works again");
            failures = 0;
        }
        let Some(id) = session.admit(&stream) else {
            continue;
        };
        if let Err(err) = session.start(id, stream) {
            session.forget(id);
            tracing::warn!(listener, error = %crate::log_file::Cause(&err), "no thread for a connection");
        }
    }
}

/// The pause after `failures` failures in a row: 10 ms, doubling, at most 1 s.
pub(super) fn backoff(failures: u64) -> Duration {
    let doublings = u32::try_from(failures.min(16)).unwrap_or(16);
    BACKOFF_FIRST
        .saturating_mul(1u32 << doublings)
        .min(BACKOFF_MOST)
}

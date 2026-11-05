//! The accept loop. A failed accept that lasts (out of file descriptors, say) is retried after
//! a pause that doubles from [`BACKOFF_FIRST`] to [`BACKOFF_MOST`] on the host's clock, and is
//! logged once per streak, with the streak's length when it ends, so it neither spins a core nor
//! floods the log.

use std::io;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use super::{Phase, Session, conn};

/// The pause after the first failed accept of a streak.
pub(super) const BACKOFF_FIRST: Duration = Duration::from_millis(10);
/// The longest pause.
pub(super) const BACKOFF_MOST: Duration = Duration::from_secs(1);

/// Accepts with `accept` until the session is stopped (the stop wakes it with a connection).
pub(super) fn accept_loop(
    session: &Arc<Session>,
    mut accept: impl FnMut() -> io::Result<TcpStream>,
) {
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
                    tracing::warn!(error = %crate::log_file::Cause(&err), "case-law relay: accept failed; retrying with a pause");
                }
                let pause = backoff(failures);
                failures = failures.saturating_add(1);
                futures::executor::block_on(session.hub.clock.after(pause));
                continue;
            }
        };
        if failures > 0 {
            tracing::warn!(failures, "case-law relay: accept works again");
            failures = 0;
        }
        let Some(id) = session.admit(&stream) else {
            continue;
        };
        if let Err(err) = conn::start(session, id, stream) {
            session.forget(id);
            tracing::warn!(error = %crate::log_file::Cause(&err), "case-law relay: no thread for a connection");
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

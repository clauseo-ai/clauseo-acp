//! The registry's held responses: a stall waits for a release, a trickle sends a byte at a
//! time; both wait on the registry's condvar, so its drop ends them.

use std::io::{self, Write};
use std::sync::PoisonError;

use super::Shared;

/// The wait between two bytes of a [`super::Behaviour::Trickle`] body.
pub const TRICKLE: std::time::Duration = std::time::Duration::from_millis(200);

/// Counts a stall, then sends `rest` one byte per [`TRICKLE`], waiting on the condvar so the
/// registry's drop ends it at once; a client gone ends it with the write's error.
pub(super) fn trickle(shared: &Shared, stream: &mut impl Write, rest: &[u8]) -> io::Result<()> {
    {
        let mut state = shared.lock();
        state.stalled += 1;
        shared.changed.notify_all();
    }
    for byte in rest {
        let state = shared.lock();
        let (state, _timeout) = shared
            .changed
            .wait_timeout_while(state, TRICKLE, |state| !state.stopped)
            .unwrap_or_else(PoisonError::into_inner);
        if state.stopped {
            return Ok(());
        }
        drop(state);
        stream.write_all(std::slice::from_ref(byte))?;
        stream.flush()?;
    }
    Ok(())
}

/// Counts a stall, then waits for the next release (or the registry's drop).
pub(super) fn stall(shared: &Shared) {
    let mut state = shared.lock();
    let seen = state.releases;
    state.stalled += 1;
    shared.changed.notify_all();
    while state.releases == seen {
        state = shared
            .changed
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

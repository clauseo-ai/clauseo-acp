//! F4's live session: an open marker whose outer still runs, told from a dead outer's by the
//! outer's pid and creation time.

use super::super::marker::{MarkerState, Outer, UncleanSession};
use super::Markers;

/// What opening a marker's outer by its pid found (`unclean/os.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuterSeen {
    /// It opened, hasn't exited, and was created at `created`.
    Running {
        /// Its creation time, as [`Outer::created`] holds it.
        created: u64,
    },
    /// It opened but has exited: a handle someone still holds keeps its object.
    Exited,
    /// No process has the pid.
    Gone,
    /// The open was refused. The outer runs as the lawyer, who can open every outer, so a
    /// process the lawyer can't open isn't one.
    Denied,
    /// Any other failure, of the open or of a read through it.
    Failed,
}

/// Whether `outer` still runs, strictly: a process opened by its pid that hasn't exited and
/// whose creation time is the marker's. Anything else (`seen` gone, refused, failed, exited, or
/// created at another time, a later process given the same pid) is a dead outer.
pub fn outer_alive(seen: OuterSeen, outer: Outer) -> bool {
    seen == OuterSeen::Running {
        created: outer.created,
    }
}

/// The id and outer pid of `session` when it is open and its outer still runs.
pub(super) fn live_outer(fs: &mut impl Markers, session: &UncleanSession) -> Option<(String, u32)> {
    let outer = session
        .outer
        .filter(|_| session.state == Some(MarkerState::Open))?;
    outer_alive(fs.outer(outer.pid), outer).then(|| (session.id.clone(), outer.pid))
}

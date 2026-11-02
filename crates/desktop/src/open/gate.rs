//! Limits on the commands script in the page could call in a loop: one link confirm at a time,
//! and none for a moment after a decline, so a page cannot stack confirms or re-open one the
//! moment it is cancelled; one `open_file` at a time, and at most one a second.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use clauseo_protocol::{ErrorCode, HostError};

use crate::bridge::lock;

/// How long after a declined confirm another is refused.
pub(crate) const AFTER_DECLINE: Duration = Duration::from_secs(1);

/// The least time between two `open_file` calls.
pub(crate) const BETWEEN_OPENS: Duration = Duration::from_secs(1);

/// The clock the gates read: [`now`] in the app, a hand-moved one in tests.
pub(crate) type Now<'a> = &'a (dyn Fn() -> Instant + Sync);

/// The monotonic clock, for the gates in the app.
#[expect(
    clippy::disallowed_methods,
    reason = "the desktop shell's rate limits read the clock; the core crates still take it as input"
)]
pub(crate) fn now() -> Instant {
    Instant::now()
}

/// One command's gate: whether a call is in progress, and until when the next is refused.
#[derive(Debug, Default)]
pub(crate) struct Gate {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    busy: bool,
    quiet_until: Option<Instant>,
}

/// A call let through: the gate is busy until it drops.
#[derive(Debug)]
pub(crate) struct Pass<'a> {
    gate: &'a Gate,
}

impl Gate {
    /// Lets one call through at `now`: `command.conflict` while another is in progress or
    /// before the quiet time an earlier call set has passed.
    pub(crate) fn enter(&self, now: Instant) -> Result<Pass<'_>, HostError> {
        let mut state = lock(&self.state);
        if state.busy {
            return Err(HostError::new(
                ErrorCode::CommandConflict,
                "another call of this command is in progress",
            ));
        }
        if state.quiet_until.is_some_and(|until| now < until) {
            return Err(HostError::new(
                ErrorCode::CommandConflict,
                "this command was called a moment ago; try again shortly",
            ));
        }
        state.busy = true;
        Ok(Pass { gate: self })
    }
}

impl Pass<'_> {
    /// Refuses the next call until `now + quiet`.
    pub(crate) fn quiet_for(&self, now: Instant, quiet: Duration) {
        lock(&self.gate.state).quiet_until = Some(now + quiet);
    }
}

impl Drop for Pass<'_> {
    fn drop(&mut self) {
        lock(&self.gate.state).busy = false;
    }
}

/// The gates of `open_external` and `open_file`, managed app state.
#[derive(Debug, Default)]
pub(crate) struct OpenGates {
    /// `open_external`'s confirm.
    pub(crate) links: Gate,
    /// `open_file`.
    pub(crate) files: Gate,
}

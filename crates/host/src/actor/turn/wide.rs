//! What the turn path keeps for the whole host, beside each thread's own state.

use super::idle::IdleClock;
use super::launch_checks::RunSweep;
use super::sign_in::StopWaits;

/// The turn path's host-wide state.
#[derive(Debug, Default)]
pub(crate) struct TurnPath {
    /// The start-up sweep of leftover `run/` folders, done by the first launch's prep.
    pub(crate) run_sweep: RunSweep,
    /// The `stop_agents` calls waiting for their agents' ends (`sign_in/stops.rs`).
    pub(super) stops: StopWaits,
    /// The idle stop's settings, live agents and its one deadline (`idle.rs`).
    pub(super) idle: IdleClock,
    /// The launch preps waiting for a browser slot to come free (`launch_checks::browser`).
    pub(super) slot_waits: Vec<futures::channel::oneshot::Sender<()>>,
}

impl TurnPath {
    /// The turn path's state at start-up, with the idle stop's settings.
    pub(crate) fn new(idle: crate::config::IdleStop) -> Self {
        Self {
            idle: IdleClock::new(idle),
            ..Self::default()
        }
    }
}

//! What the turn path keeps for the whole host, beside each thread's own state.

use super::launch_checks::RunSweep;
use super::sign_in::StopWaits;

/// The turn path's host-wide state.
#[derive(Debug, Default)]
pub(crate) struct TurnPath {
    /// The start-up sweep of leftover `run/` folders, done by the first launch's prep.
    pub(crate) run_sweep: RunSweep,
    /// The `stop_agents` calls waiting for their agents' ends (`sign_in/stops.rs`).
    pub(super) stops: StopWaits,
}

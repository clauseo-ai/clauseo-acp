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
    /// Each hears whether a slot is free once a holder's removal ends.
    pub(super) slot_waits: Vec<futures::channel::oneshot::Sender<bool>>,
    /// The window's last `SetNetwork` said the computer is offline (`offline.rs`).
    pub(super) offline: bool,
    /// Whether the agent's service answers, asked before a watch only the window armed ends
    /// a turn (`offline/probe.rs`); tests set a fixed answer.
    pub(super) probe: super::offline::probe::ReachProbe,
    /// Whether a live turn of an agent with a known service is watched for silence
    /// (`offline/quiet.rs`): on, but off in unit tests unless a test turns it on.
    pub(super) quiet_watch: bool,
    /// The kind the unit tests' `fake-agent` plays for the offline probe and the quiet arm.
    #[cfg(test)]
    pub(super) kind_for_tests: Option<clauseo_protocol::AgentKind>,
}

impl TurnPath {
    /// The turn path's state at start-up, with the idle stop's settings and the quiet watch's
    /// switch (off in unit tests whatever the config says, unless a test turns it on).
    pub(crate) fn new(idle: crate::config::IdleStop) -> Self {
        Self {
            idle: IdleClock::new(idle),
            quiet_watch: idle.quiet_watch && !cfg!(test),
            ..Self::default()
        }
    }
}

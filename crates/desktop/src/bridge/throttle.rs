//! The settings window's `refresh_limits` throttle (0028's 2026-11-03 amendment): at most one
//! per agent (or one for every agent, `agent: None`) per [`REFRESH_GAP`]. Plan and usage's Try
//! again pressed over and over would otherwise have the host start a read for each press. A
//! throttled command is dropped quietly: `dispatch` answers `Done` and sends the host nothing,
//! since no error code means "too soon" and the page's next limits view says what came of the
//! last read anyway. `main`'s refreshes pass untouched; the host's own throttle (#220) will cover
//! every window, and this stays as defence in depth.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use clauseo_protocol::AgentKind;

use super::lock;

/// The least time between two of the settings window's refreshes of one agent.
pub(super) const REFRESH_GAP: Duration = Duration::from_secs(10);

/// When each agent's last refresh from the settings window passed.
#[derive(Default)]
pub(super) struct RefreshThrottle {
    passed: Mutex<HashMap<Option<AgentKind>, Instant>>,
}

impl RefreshThrottle {
    /// Whether a refresh of `agent` at `now` passes: none passed in the [`REFRESH_GAP`] before
    /// it. One that passes is recorded; one that does not leaves the record as it was.
    pub(super) fn pass(&self, agent: Option<AgentKind>, now: Instant) -> bool {
        let mut passed = lock(&self.passed);
        match passed.get(&agent) {
            Some(last) if now.saturating_duration_since(*last) < REFRESH_GAP => false,
            _ => {
                passed.insert(agent, now);
                true
            }
        }
    }
}

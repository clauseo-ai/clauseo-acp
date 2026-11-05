//! What piece B adds to the setup backend: the limits readers it holds and swaps entries in,
//! the reused logins the host's login check reads, the waiters on S18's states, the plans
//! snapshot the launch hook reads, and what it last took from the settings.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_protocol::AgentId;

use super::hook::Plans;
use super::leave::{LEAVE_WAIT, Waiters};
use super::login_check::ReusedLogins;
use super::readers::LeaveReaders;
use super::refresh::SettingsRead;

/// Piece B's part of [`AgentsBackend`](super::backend::AgentsBackend).
pub(crate) struct Wiring {
    pub(crate) readers: Arc<dyn LeaveReaders>,
    pub(crate) reused: ReusedLogins,
    pub(crate) waiters: Waiters,
    /// How long the leave waits for S18's `SignedOut`.
    pub(crate) leave_wait: Duration,
    pub(crate) plans: Plans,
    /// The settings as last taken in (`refresh::apply_settings`).
    pub(crate) applied: Mutex<SettingsRead>,
    /// The agents whose new launches the host was told to refuse.
    pub(crate) refused: Mutex<BTreeSet<AgentId>>,
}

impl Wiring {
    /// Over `readers`, `reused` and `plans`, with `start` the settings the backend's facts were
    /// set from; the host has been told no refusal yet.
    pub(crate) fn new(
        readers: Arc<dyn LeaveReaders>,
        reused: ReusedLogins,
        plans: Plans,
        start: SettingsRead,
    ) -> Self {
        Self {
            readers,
            reused,
            waiters: Waiters::default(),
            leave_wait: LEAVE_WAIT,
            plans,
            applied: Mutex::new(start),
            refused: Mutex::default(),
        }
    }
}

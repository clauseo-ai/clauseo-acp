//! What piece B adds to the setup backend: the limits readers it holds and swaps entries in,
//! the reused logins the host's login check reads, the waiters on S18's states, the running
//! leaves and the leave's timer, the plans
//! snapshot the launch hook reads, and what it last took from the settings.

use clauseo_protocol::AgentId;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use super::hook::Plans;
use clauseo_process::AsyncIoTimer;

use super::leave::{LeaveTimer, Leaves, Waiters};
use super::login_check::ReusedLogins;
use super::readers::LeaveReaders;
use super::refresh::SettingsRead;

/// Piece B's part of [`AgentsBackend`](super::backend::AgentsBackend).
pub(crate) struct Wiring {
    pub(crate) readers: Arc<dyn LeaveReaders>,
    pub(crate) reused: ReusedLogins,
    pub(crate) waiters: Waiters,
    /// The running leave of each agent, and the automatic leaves' queue.
    pub(crate) leaves: Leaves,
    /// The leave's waits: S18's `SignedOut`, the tries again, `stuck`.
    pub(crate) timer: Arc<dyn LeaveTimer>,
    pub(crate) plans: Plans,
    /// The settings as last taken in (`refresh::apply_settings`).
    pub(crate) applied: Mutex<SettingsRead>,
    /// The agents whose new launches the host was told to refuse.
    pub(crate) refused: Mutex<BTreeSet<AgentId>>,
}

impl Wiring {
    /// Over `readers`, `reused` and `plans`, with `start` the settings the backend's facts were
    /// set from; the host started refusing `start`'s blocked agents
    /// (`SetupStart::launch_refusals`).
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
            leaves: Leaves::default(),
            timer: Arc::new(AsyncIoTimer),
            plans,
            refused: Mutex::new(start.blocked.clone()),
            applied: Mutex::new(start),
        }
    }

    /// The leave's waits on `timer` (the tests' hand-driven one).
    #[cfg(test)]
    pub(crate) fn with_timer(mut self, timer: Arc<dyn LeaveTimer>) -> Self {
        self.timer = timer;
        self
    }
}

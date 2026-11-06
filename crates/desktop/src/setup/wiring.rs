//! What piece B adds to the setup backend: the limits readers it holds and swaps entries in,
//! the reused logins the host's login check reads, the waiters on S18's states, the running
//! leaves and the leave's timer, the plans
//! snapshot the launch hook reads, and what it last took from the settings.

use clauseo_protocol::{AgentId, ErrorCode};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::hook::Plans;
use super::identity::Identities;
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
    /// The code the host was last told to refuse each agent's new launches with; an agent not
    /// here may launch (`setup/refusal.rs`).
    pub(crate) refused: Mutex<BTreeMap<AgentId, ErrorCode>>,
    /// Held across each change of a refusal, so the host's and [`Self::refused`] keep step.
    pub(crate) refusal_gate: futures::lock::Mutex<()>,
    /// Each agent's login's account and its check (R17, `setup/identity.rs`).
    pub(crate) identity: Identities,
}

impl Wiring {
    /// Over `readers`, `reused` and `plans`, with `start` the settings the backend's facts were
    /// set from and `refused` the refusals the host started with
    /// (`SetupStart::launch_refusals`).
    pub(crate) fn new(
        readers: Arc<dyn LeaveReaders>,
        reused: ReusedLogins,
        plans: Plans,
        start: SettingsRead,
        refused: impl IntoIterator<Item = (AgentId, ErrorCode)>,
    ) -> Self {
        Self {
            readers,
            reused,
            waiters: Waiters::default(),
            leaves: Leaves::default(),
            timer: Arc::new(AsyncIoTimer),
            plans,
            refused: Mutex::new(refused.into_iter().collect()),
            refusal_gate: futures::lock::Mutex::new(()),
            applied: Mutex::new(start),
            identity: Identities::in_memory(),
        }
    }

    /// The identity check over `identity` (its record read from the data folder).
    pub(crate) fn with_identity(mut self, identity: Identities) -> Self {
        self.identity = identity;
        self
    }

    /// The leave's waits on `timer` (the tests' hand-driven one).
    #[cfg(test)]
    pub(crate) fn with_timer(mut self, timer: Arc<dyn LeaveTimer>) -> Self {
        self.timer = timer;
        self
    }
}

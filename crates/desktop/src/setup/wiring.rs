//! What piece B adds to the setup backend: the limits readers it holds and swaps entries in,
//! the reused logins the host's login check reads, the waiters on S18's states, the running
//! leaves and the leave's timer, the plans
//! snapshot the launch hook reads, what it last took from the settings, and where the network
//! route is resolved.

use clauseo_protocol::{AgentId, ErrorCode};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::hook::Plans;
use clauseo_process::AsyncIoTimer;

use super::leave::{LeaveTimer, Leaves, Waiters};
use super::login_check::ReusedLogins;
use super::network::NetworkFollow;
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
    /// Where each settings read's network facts go (`setup/network.rs`): none until the setup
    /// thread sets it.
    pub(crate) network: NetworkFollow,
    /// The hosts the Python runtime's download contacts, for its `Waiting` and blocked
    /// `Failed` (`backend/entry.rs`); none until the setup thread sets them.
    pub(crate) python_hosts: Vec<String>,
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
            network: NetworkFollow::default(),
            python_hosts: Vec::new(),
        }
    }

    /// Python's download hosts are `hosts`.
    #[must_use]
    pub(crate) fn with_python_hosts(mut self, hosts: Vec<String>) -> Self {
        self.python_hosts = hosts;
        self
    }

    /// The network facts of each settings read go to `network`.
    #[must_use]
    pub(crate) fn with_network(mut self, network: NetworkFollow) -> Self {
        self.network = network;
        self
    }

    /// The leave's waits on `timer` (the tests' hand-driven one).
    #[cfg(test)]
    pub(crate) fn with_timer(mut self, timer: Arc<dyn LeaveTimer>) -> Self {
        self.timer = timer;
        self
    }
}

//! The leave's waits, and the host's hold a lawyer's leave lifts whatever happens.
//!
//! **Waits** ([`LeaveTimer`]): the 30 s wait for S18's `SignedOut`, an automatic leave's tries
//! again (5 s, 30 s, then every 5 minutes, [`retry_after`]) and its 2 minutes to `stuck`. They run
//! on the timer S18's sign-in runs on (its `Waiting { nudge }` at 30 s), `AsyncIoTimer`, behind
//! one seam the tests drive by hand.
//!
//! **The hold** ([`HostHold`]): from `stop_agents` on, the host holds the agent until
//! `restart_agents`. A lawyer's leave that does not finish lifts it ([`HostHold::lift`]); one
//! dropped before it ends (a panic, or its command's future dropped) lifts it from its `Drop`,
//! on a thread of its own (review L6). A failed `restart_agents` is tried once more, then logged.

use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{AsyncIoTimer, Timer};
use clauseo_protocol::AgentId;

use super::super::host_calls::SetupHost;
use crate::dialogs::BoxFuture;

/// How long the leave waits for S18's `SignedOut` after `stop_using_login`.
pub(crate) const LEAVE_WAIT: Duration = Duration::from_secs(30);

/// How long after its first try's end an automatic leave shows `stuck`.
pub(crate) const STUCK_AFTER: Duration = Duration::from_secs(120);

/// The waits between an automatic leave's tries: then the last, for ever.
pub(crate) const RETRIES: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(300),
];

/// The wait before try again number `n` (from 0).
pub(crate) fn retry_after(n: usize) -> Duration {
    RETRIES[n.min(RETRIES.len() - 1)]
}

/// One of the leave's waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wait {
    /// For S18's `SignedOut` ([`LEAVE_WAIT`]).
    SignedOut,
    /// Before an automatic leave tries again.
    Retry(Duration),
    /// Until an automatic leave shows `stuck` ([`STUCK_AFTER`]).
    Stuck,
}

impl Wait {
    /// How long it lasts.
    pub(crate) fn duration(self) -> Duration {
        match self {
            Self::SignedOut => LEAVE_WAIT,
            Self::Retry(after) => after,
            Self::Stuck => STUCK_AFTER,
        }
    }
}

/// The leave's timer.
pub(crate) trait LeaveTimer: Send + Sync {
    /// A future that ends once `wait` has passed.
    fn after(&self, wait: Wait) -> BoxFuture<'static, ()>;
}

impl LeaveTimer for AsyncIoTimer {
    fn after(&self, wait: Wait) -> BoxFuture<'static, ()> {
        let timer = *self;
        Box::pin(async move { Timer::after(&timer, wait.duration()).await })
    }
}

/// The host's hold on one agent from `stop_agents` on, lifted on drop unless kept.
pub(crate) struct HostHold {
    host: Arc<dyn SetupHost>,
    agent: AgentId,
    armed: bool,
}

impl HostHold {
    /// The hold `stop_agents` is about to take for `agent`.
    pub(crate) fn new(host: Arc<dyn SetupHost>, agent: AgentId) -> Self {
        Self {
            host,
            agent,
            armed: true,
        }
    }

    /// The leave finished: the hold stays until the new login is in place.
    pub(crate) fn keep(mut self) {
        self.armed = false;
    }

    /// The leave did not finish: `restart_agents` lifts the hold.
    pub(crate) async fn lift(mut self) {
        self.armed = false;
        lift(&*self.host, &self.agent).await;
    }
}

impl Drop for HostHold {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (host, agent) = (Arc::clone(&self.host), self.agent.clone());
        tracing::warn!(%agent, "a leave ended early: lifting the agents' hold");
        let spawned = std::thread::Builder::new()
            .name("clauseo-setup-lift".to_owned())
            .spawn(move || futures::executor::block_on(lift(&*host, &agent)));
        if let Err(err) = spawned {
            let agent = &self.agent;
            tracing::warn!(%agent, %err, "the agents' hold not lifted: no thread");
        }
    }
}

/// `restart_agents` for `agent`, tried once more when it fails, then logged by code.
pub(crate) async fn lift(host: &dyn SetupHost, agent: &AgentId) {
    for again in [false, true] {
        match host.restart_agents(agent.clone()).await {
            Ok(()) => return,
            Err(error) if !again => {
                tracing::info!(%agent, code = error.code().as_str(), "restart failed; again");
            }
            Err(error) => {
                tracing::warn!(%agent, code = error.code().as_str(), "the agents' hold not lifted");
            }
        }
    }
}

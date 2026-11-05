//! One leave per agent (review H1), and the queue of automatic leaves.
//!
//! A leave takes its agent's [`Slot`] for its whole run; a second leave of that agent while one
//! runs is refused with `command.conflict`. An automatic leave refused so is kept beside the
//! running one: when that one ends without leaving (a lawyer's leave given up), the automatic
//! leave is queued again; when it left the login, or a new sign-in ended it, the automatic leave
//! has nothing left to do and goes. Automatic leaves ([`AutoLeave`]: a split, or a reuse S18
//! ended by itself) are queued here and run, each on its own, by `auto::follow`, never inside
//! the pump.

use std::collections::BTreeMap;
use std::sync::Mutex;

use clauseo_agents::Found;
use clauseo_protocol::{AgentId, ErrorCode, HostError};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures::channel::oneshot;

use crate::bridge::lock;

/// An automatic leave: the agent, and the reason S18 is given (`stop_using_login(why)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoLeave {
    pub(crate) agent: AgentId,
    pub(crate) why: Option<Found>,
}

/// The running leave of each agent, and the queue of automatic leaves.
pub(crate) struct Leaves {
    running: Mutex<BTreeMap<AgentId, Running>>,
    queue: UnboundedSender<AutoLeave>,
    taken: Mutex<Option<UnboundedReceiver<AutoLeave>>>,
}

/// An automatic leave kept for later, with its reason.
struct Deferred(Option<Found>);

/// One running leave.
#[derive(Default)]
struct Running {
    /// An automatic leave refused while this one ran.
    deferred: Option<Deferred>,
    /// Told when a new sign-in arrives for the agent.
    signed_in: Option<oneshot::Sender<()>>,
}

impl Default for Leaves {
    fn default() -> Self {
        let (queue, taken) = unbounded();
        Self {
            running: Mutex::default(),
            queue,
            taken: Mutex::new(Some(taken)),
        }
    }
}

impl Leaves {
    /// `agent`'s slot, or `command.conflict` while another leave of it runs.
    pub(crate) fn begin(&self, agent: &AgentId) -> Result<Slot<'_>, HostError> {
        let mut running = lock(&self.running);
        if running.contains_key(agent) {
            return Err(HostError::new(
                ErrorCode::CommandConflict,
                format!("{agent}'s login is already being left"),
            ));
        }
        running.insert(agent.clone(), Running::default());
        Ok(Slot {
            leaves: self,
            agent: agent.clone(),
            left: false,
        })
    }

    /// Keeps an automatic leave of `agent` refused while another leave of it runs.
    pub(crate) fn defer(&self, agent: &AgentId, why: Option<Found>) {
        let mut running = lock(&self.running);
        if let Some(leave) = running.get_mut(agent) {
            leave.deferred = Some(Deferred(why));
            return;
        }
        drop(running);
        self.submit(agent, why);
    }

    /// Queues an automatic leave of `agent`.
    pub(crate) fn submit(&self, agent: &AgentId, why: Option<Found>) {
        let leave = AutoLeave {
            agent: agent.clone(),
            why,
        };
        if self.queue.unbounded_send(leave).is_err() {
            tracing::warn!(%agent, "an automatic leave was not queued");
        }
    }

    /// A new sign-in arrived for `agent`: its running automatic leave, if any, stops trying.
    pub(crate) fn signed_in(&self, agent: &AgentId) {
        let tell = lock(&self.running)
            .get_mut(agent)
            .and_then(|leave| leave.signed_in.take());
        if let Some(tell) = tell {
            let _gone = tell.send(());
        }
    }

    /// The queue of automatic leaves, once: its one follower takes it.
    pub(crate) fn queue(&self) -> Option<UnboundedReceiver<AutoLeave>> {
        lock(&self.taken).take()
    }
}

/// A running leave of one agent; the agent is free again when it drops.
pub(crate) struct Slot<'a> {
    leaves: &'a Leaves,
    agent: AgentId,
    left: bool,
}

impl Slot<'_> {
    /// The login was left, or a new sign-in took its place: a deferred automatic leave goes.
    pub(crate) fn left(&mut self) {
        self.left = true;
    }

    /// Resolves when a new sign-in arrives for the agent.
    pub(crate) fn on_signed_in(&self) -> oneshot::Receiver<()> {
        let (tell, told) = oneshot::channel();
        if let Some(leave) = lock(&self.leaves.running).get_mut(&self.agent) {
            leave.signed_in = Some(tell);
        }
        told
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        let leave = lock(&self.leaves.running).remove(&self.agent);
        if let Some(Running {
            deferred: Some(Deferred(why)),
            ..
        }) = leave
        {
            if self.left {
                tracing::info!(agent = %self.agent, "an automatic leave was done by another");
            } else {
                self.leaves.submit(&self.agent, why);
            }
        }
    }
}

//! One leave per agent (review H1), and the queue of automatic leaves.
//!
//! A leave takes its agent's [`Slot`] for its whole run; a second leave of that agent while one
//! runs is refused with `command.conflict`. An automatic leave refused so is kept beside the
//! running one: when that one ends without leaving (a lawyer's leave that failed), the automatic
//! leave is queued again; when it left the login, or a new sign-in ended it, the automatic leave
//! has nothing left to do and goes. A lawyer's leave that gave up keeps its slot
//! ([`Slot::linger`]) until S18's stop job ends ([`Leaves::end_late`]).
//!
//! Automatic leaves ([`AutoLeave`]: a split, or a reuse S18 ended by itself) are queued here
//! and run, each on its own, by `auto::follow`, never inside the pump.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use clauseo_agents::Found;
use clauseo_protocol::{AgentId, ErrorCode, HostError};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures::channel::oneshot;

use super::hold::lift;
use crate::bridge::lock;
use crate::setup::host_calls::SetupHost;

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
    /// The one point of a leave's end a test stops at ([`Leaves::pause`]).
    #[cfg(test)]
    pause: Mutex<Option<Pause>>,
}

/// A test's stop at a point of a leave's end: told when it is reached, then it waits for `go`.
#[cfg(test)]
struct Pause {
    at: &'static str,
    reached: oneshot::Sender<()>,
    go: oneshot::Receiver<()>,
}

/// An automatic leave kept for later, with its reason.
#[derive(Clone, Copy)]
struct Deferred(Option<Found>);

/// One running leave.
#[derive(Default)]
struct Running {
    /// An automatic leave refused while this one ran.
    deferred: Option<Deferred>,
    /// Told when a new sign-in arrives for the agent.
    signed_in: Option<oneshot::Sender<()>>,
    /// Whether a new sign-in arrived since the leave began.
    told: bool,
    /// Whether an endpoint arrived for the agent since the leave's last restart: `apply_settings`
    /// skipped its restart, and the leave restarts when it frees its slot
    /// ([`Leaves::endpoint_arrived`], [`Leaves::covered_by_restart`]).
    endpoint_arrived: bool,
}

impl Default for Leaves {
    fn default() -> Self {
        let (queue, taken) = unbounded();
        Self {
            running: Mutex::default(),
            queue,
            taken: Mutex::new(Some(taken)),
            #[cfg(test)]
            pause: Mutex::default(),
        }
    }
}

impl Leaves {
    /// `agent`'s slot, or `command.conflict` while another leave of it runs. `host` restarts
    /// the agent should the slot drop with an endpoint's arrival not taken ([`Slot::free`]).
    pub(crate) fn begin(
        &self,
        agent: &AgentId,
        host: &Arc<dyn SetupHost>,
    ) -> Result<Slot<'_>, HostError> {
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
            host: Arc::clone(host),
            left: false,
            lingers: false,
            freed: false,
        })
    }

    /// The stop job a lawyer's leave gave up on ended (`left`: the reuse stopped): the agent
    /// is free again, and a deferred automatic leave goes as when a leave ends. True when an
    /// endpoint arrived for the agent meanwhile: the caller restarts it by the rule.
    pub(crate) fn end_late(&self, agent: &AgentId, left: bool) -> bool {
        self.end(agent, left)
    }

    /// Takes `agent`'s running leave off, and reads its endpoint flag in the same critical
    /// section, so an arrival is either seen here or restarted by `apply_settings`. True when
    /// an endpoint arrived while the leave ran.
    fn end(&self, agent: &AgentId, left: bool) -> bool {
        let leave = lock(&self.running).remove(agent);
        let arrived = leave.as_ref().is_some_and(|leave| leave.endpoint_arrived);
        self.ended(agent, deferred(leave), left);
        arrived
    }

    /// A leave of `agent` ended: `deferred`, the reason of an automatic leave kept beside it, is
    /// queued again unless `left`.
    fn ended(&self, agent: &AgentId, deferred: Option<Deferred>, left: bool) {
        if let Some(Deferred(why)) = deferred {
            if left {
                tracing::info!(%agent, "an automatic leave was done by another");
            } else {
                self.submit(agent, why);
            }
        }
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
        let tell = lock(&self.running).get_mut(agent).and_then(|leave| {
            leave.told = true;
            leave.signed_in.take()
        });
        if let Some(tell) = tell {
            let _gone = tell.send(());
        }
    }

    /// An endpoint arrived for `agent` (`apply_settings`). Under the lock a running leave of it
    /// reads ([`Slot`], [`Leaves::end_late`]), the arrival is recorded on that leave, and true
    /// says so: the leave restarts the agent when it frees its slot. False when no leave of
    /// `agent` runs, and the caller restarts it.
    pub(crate) fn endpoint_arrived(&self, agent: &AgentId) -> bool {
        let mut running = lock(&self.running);
        let Some(leave) = running.get_mut(agent) else {
            return false;
        };
        leave.endpoint_arrived = true;
        true
    }

    /// A restart of `agent` is about to be told by its hold ([`HostHold::lift`]): it covers an
    /// endpoint that arrived before it, so the arrival flag clears under the lock. An arrival
    /// after this sets the flag again, and the leave then restarts at its free.
    pub(crate) fn covered_by_restart(&self, agent: &AgentId) {
        if let Some(leave) = lock(&self.running).get_mut(agent) {
            leave.endpoint_arrived = false;
        }
    }

    /// A test stops at `at` (a point of a leave's end, [`Leaves::pause`]): the returned receiver
    /// is told when a leave reaches it, and the sender lets it go on.
    #[cfg(test)]
    pub(crate) fn arm_pause(
        &self,
        at: &'static str,
    ) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (reached, told) = oneshot::channel();
        let (go, wait) = oneshot::channel();
        *lock(&self.pause) = Some(Pause {
            at,
            reached,
            go: wait,
        });
        (told, go)
    }

    /// A leave reaches `at`: a test armed there waits for its go.
    #[cfg(test)]
    pub(crate) async fn pause(&self, at: &'static str) {
        let armed = lock(&self.pause).take_if(|pause| pause.at == at);
        if let Some(Pause { reached, go, .. }) = armed {
            let _told = reached.send(());
            let _go = go.await;
        }
    }

    /// Whether a new sign-in arrived for `agent` since its running leave began: the pump has
    /// handled it, `restart_agents` included.
    pub(crate) fn told_signed_in(&self, agent: &AgentId) -> bool {
        lock(&self.running)
            .get(agent)
            .is_some_and(|leave| leave.told)
    }

    /// The queue of automatic leaves, once: its one follower takes it.
    pub(crate) fn queue(&self) -> Option<UnboundedReceiver<AutoLeave>> {
        lock(&self.taken).take()
    }
}

/// The reason of the automatic leave kept beside `leave`, if any.
fn deferred(leave: Option<Running>) -> Option<Deferred> {
    leave.and_then(|leave| leave.deferred)
}

/// A running leave of one agent; the agent is free again when it drops, or when [`Slot::free`]
/// is called.
pub(crate) struct Slot<'a> {
    leaves: &'a Leaves,
    agent: AgentId,
    host: Arc<dyn SetupHost>,
    left: bool,
    /// Kept past its drop, until [`Leaves::end_late`].
    lingers: bool,
    /// Freed by [`Slot::free`]: the drop does nothing.
    freed: bool,
}

impl Slot<'_> {
    /// A lawyer's leave gave up while S18's stop job may still run: the slot stays taken until
    /// the pump sees the job end.
    pub(crate) fn linger(mut self) {
        self.lingers = true;
    }

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

    /// Frees the agent, the leave's last step. True when an endpoint arrived for it while the
    /// slot was taken: the leave then restarts the agent by the rule (`restart_if_usable`).
    pub(crate) fn free(mut self) -> bool {
        self.freed = true;
        self.leaves.end(&self.agent, self.left)
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        if self.lingers || self.freed {
            return;
        }
        if self.leaves.end(&self.agent, self.left) {
            // Dropped before its own free, with an endpoint's arrival not taken: restarted here,
            // without the usable check, as the hold's drop does (no backend is in reach).
            restart_on_drop(&self.host, &self.agent);
        }
    }
}

/// `restart_agents` for `agent` on a thread of its own, from a drop.
fn restart_on_drop(host: &Arc<dyn SetupHost>, agent: &AgentId) {
    let (host, for_thread) = (Arc::clone(host), agent.clone());
    let spawned = std::thread::Builder::new()
        .name("clauseo-setup-restart".to_owned())
        .spawn(move || futures::executor::block_on(lift(&*host, &for_thread)));
    if let Err(err) = spawned {
        tracing::warn!(%agent, %err, "an endpoint arrived in a leave, not restarted: no thread");
    }
}

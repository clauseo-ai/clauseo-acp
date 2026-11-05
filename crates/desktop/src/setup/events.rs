//! Where the setup backend publishes [`SetupEvent`]s and the windows' channels follow them.
//! Each event is encoded once, as the channel's `setup` message
//! (`{"setup":{"sign_in_changed":...}}`, `frame::setup_text`), and handed to every follower.
//!
//! It keeps the latest `setup_info` and each agent's latest install and sign-in state, so a new
//! follower (a `subscribe_app`, or a feed restarted after a pause or the watchdog) first gets
//! `setup_info`, then each agent's install and sign-in state, before any newer event (0048).
//! Publishing and following take one lock, so nothing published falls between the two.
//!
//! Setup events never reach a log: they hold emails, URLs, device codes and install logs. A
//! sign-in note (S18's `SigninNote`) is logged with [`note`], by its code.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use clauseo_protocol::{AgentId, SetupEvent, SignIn};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::bridge::lock;

/// The setup events' publisher and the windows' feed of them.
#[derive(Default)]
pub(crate) struct SetupEvents {
    state: Mutex<Latest>,
}

/// The latest state, and the followers.
#[derive(Default)]
struct Latest {
    /// The latest `setup_info`, encoded.
    info: Option<Arc<str>>,
    /// Each agent's latest states, by id.
    agents: BTreeMap<AgentId, AgentLatest>,
    /// Each follower's channel; one whose receiver is gone is dropped at the next publish.
    followers: Vec<UnboundedSender<Arc<str>>>,
}

/// One agent's latest install and sign-in states, encoded.
#[derive(Default)]
struct AgentLatest {
    install: Option<Arc<str>>,
    sign_in: Option<Arc<str>>,
    /// Whether that sign-in state is a running one: `Queued`, `Waiting` or `DeviceCode`.
    running: bool,
    /// Whether a `StartSignIn` went to the backend and no running state or failure has been
    /// published since (`Setup::start_sign_in`).
    starting: bool,
}

impl SetupEvents {
    /// Publishes `event` to every follower and keeps it as the latest of its kind. An event
    /// that does not encode (none does) is dropped.
    pub(crate) fn publish(&self, event: SetupEvent) {
        let slot = Slot::of(&event);
        let Some(text) = crate::frame::setup_text(event) else {
            tracing::warn!("a setup event did not encode and was dropped");
            return;
        };
        let text: Arc<str> = text.into();
        let mut latest = lock(&self.state);
        match slot {
            Slot::Info => latest.info = Some(Arc::clone(&text)),
            Slot::Install(agent) => {
                latest.agents.entry(agent).or_default().install = Some(Arc::clone(&text));
            }
            Slot::SignIn(agent, running, failed) => {
                let entry = latest.agents.entry(agent).or_default();
                entry.sign_in = Some(Arc::clone(&text));
                entry.running = running;
                entry.starting &= !(running || failed);
            }
        }
        latest
            .followers
            .retain(|follower| follower.unbounded_send(Arc::clone(&text)).is_ok());
    }

    /// A new follower: the latest `setup_info`, then each agent's install and sign-in state,
    /// then every event published from now on, as channel message texts. Followers whose
    /// receiver is gone are dropped here, as at each publish.
    pub(crate) fn follow(&self) -> UnboundedReceiver<Arc<str>> {
        let (follower, feed) = unbounded();
        let mut latest = lock(&self.state);
        let agents = latest
            .agents
            .values()
            .flat_map(|agent| [agent.install.as_ref(), agent.sign_in.as_ref()]);
        let replay: Vec<Arc<str>> = latest
            .info
            .iter()
            .chain(agents.flatten())
            .map(Arc::clone)
            .collect();
        for text in replay {
            // The receiver is in hand: an unbounded send to it cannot fail.
            let _sent = follower.unbounded_send(text);
        }
        // A follower whose receiver is gone goes now too, not only at the next publish.
        latest.followers.retain(|follower| !follower.is_closed());
        latest.followers.push(follower);
        feed
    }

    /// How many followers it holds, closed ones not yet dropped included.
    #[cfg(test)]
    pub(crate) fn followers(&self) -> usize {
        lock(&self.state).followers.len()
    }

    /// Whether `agent`'s latest sign-in state is a running one.
    #[cfg(test)]
    pub(crate) fn sign_in_running(&self, agent: &AgentId) -> bool {
        lock(&self.state)
            .agents
            .get(agent)
            .is_some_and(|latest| latest.running)
    }

    /// Marks `agent`'s sign-in as starting and answers `true`, unless one runs or is starting
    /// (`false`: join it). The mark goes when a running state or a failure is published for
    /// `agent`, or with [`SetupEvents::end_start`]; checked and set under the one lock.
    pub(crate) fn begin_start(&self, agent: &AgentId) -> bool {
        let mut latest = lock(&self.state);
        let entry = latest.agents.entry(agent.clone()).or_default();
        if entry.running || entry.starting {
            return false;
        }
        entry.starting = true;
        true
    }

    /// Removes `agent`'s starting mark: the backend refused the start, or the call was dropped.
    pub(crate) fn end_start(&self, agent: &AgentId) {
        if let Some(entry) = lock(&self.state).agents.get_mut(agent) {
            entry.starting = false;
        }
    }
}

/// Logs a sign-in note (S18's `SigninNote`: `claude_reuse_fallback`, `codex_link_failed`) for
/// `agent` by its code alone, never an email or a path (0048, ruling 5).
pub(crate) fn note(agent: &AgentId, code: &'static str) {
    tracing::info!(%agent, note = code, "sign-in note");
}

/// Which latest state an event replaces.
enum Slot {
    Info,
    Install(AgentId),
    /// The agent, whether the state is a running one, and whether it is `Failed`.
    SignIn(AgentId, bool, bool),
}

impl Slot {
    fn of(event: &SetupEvent) -> Self {
        match event {
            SetupEvent::SetupInfo { .. } => Self::Info,
            SetupEvent::AgentInstallProgress { agent, .. } => Self::Install(agent.clone()),
            SetupEvent::SignInChanged { agent, state } => Self::SignIn(
                agent.clone(),
                matches!(
                    state,
                    SignIn::Queued | SignIn::Waiting { .. } | SignIn::DeviceCode { .. }
                ),
                matches!(state, SignIn::Failed { .. }),
            ),
        }
    }
}

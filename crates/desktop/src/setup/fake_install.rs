//! The `e2e` build's scripted install (`E2E_FAKE_INSTALL`), for the specs of a sign-in pressed
//! before its agent is installed: the e2e app installs nothing (its agents are `fake_agent`, and
//! CI has no business downloading), so a spec could never see an install end at `Ready`.
//! [`FakeInstall`] wraps the backend's seam over S18 ([`SetupAgents`]) and plays an install for
//! each agent the variable names; every other agent, and every call that is not an install's,
//! goes to S18 as usual.
//!
//! - `E2E_FAKE_INSTALL=<agent>[,<agent>]`; unset, empty or no valid id in it leaves S18 alone.
//! - Each named agent starts `Missing { download_bytes: 50 MB }`; S18's own install states for
//!   it are dropped from the pump's events.
//! - `install` plays `Downloading` (0, then half), `Installing`, then `Ready { version: e2e }`,
//!   each held [`HOLD`], with no network; an install already playing is joined, and one of a
//!   ready agent says `Ready` again, as S18's does. `retry` does nothing (it never fails).
//! - `installed` says nothing for it, so no `agents.<id>` entry is written at `Ready`: the
//!   spec's settings keep theirs (`fake_agent`, or none).
//! - Pair it with `E2E_FAKE_SIGNIN` ([`scripted`] wraps that over this), so the sign-in pending at
//!   `Ready` (`backend/pending.rs`) plays the scripted one rather than S18's.
//! - `E2E_FAKE_INSTALLED=<agent>[,<agent>]` (e.g. `claude-acp`): each named agent starts
//!   `Ready { version: e2e }` instead, with no install run: installed, so its status check runs at
//!   once and the welcome says it is ready. Its status answer is `E2E_FAKE_STATUS`'s
//!   (`fake_signin.rs`, signed out by default), and a press of Sign in plays `E2E_FAKE_SIGNIN`'s
//!   script for it. An agent named in both variables is installed.
//!
//! Contract: built only with the `e2e` feature (`setup.rs` declares the module under it, and
//! `start.rs` calls [`scripted`] under it), so no release reads the variable or fakes an install.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::{
    AgentsEvent, Clock, Found, InstallState, Installed, Method, PasteCode, SigninError, SigninState,
};
use clauseo_protocol::AgentId;
use futures::channel::mpsc;
use futures::future::ready;
use futures::stream::{BoxStream, Stream, StreamExt};

use super::backend::SetupAgents;
use super::fake_signin::merged;
use crate::bridge::lock;

/// The agents whose install is scripted.
const AGENTS_VAR: &str = "E2E_FAKE_INSTALL";
/// The agents that start installed, with no install run.
pub(crate) const INSTALLED_VAR: &str = "E2E_FAKE_INSTALLED";
/// How long each state before `Ready` lasts.
pub(crate) const HOLD: Duration = Duration::from_millis(1000);
/// What the scripted install "downloads".
pub(crate) const DOWNLOAD_BYTES: u64 = 50 * 1024 * 1024;
/// The version a scripted install ends with.
pub(crate) const VERSION: &str = "e2e";

/// The agents in `value` (`E2E_FAKE_INSTALL`'s form); an entry that is no agent id is dropped.
pub(crate) fn agents_from(value: Option<&str>) -> BTreeSet<AgentId> {
    value
        .unwrap_or_default()
        .split(',')
        .filter_map(|entry| AgentId::new(entry.trim()).ok())
        .collect()
}

/// The seam over S18 with the e2e build's scripted installs, then its scripted sign-ins
/// (`fake_signin.rs`) over those, so a sign-in kept until `Ready` plays the scripted one.
pub(crate) fn scripted(
    real: Arc<dyn SetupAgents>,
    source: impl Stream<Item = AgentsEvent> + Send + 'static,
    clock: Arc<dyn Clock>,
) -> (Arc<dyn SetupAgents>, BoxStream<'static, AgentsEvent>) {
    let (agents, source) = wrap(real, source);
    super::fake_signin::wrap(agents, source, clock)
}

/// The seam over S18 with `E2E_FAKE_INSTALL`'s installs, and the pump's events: S18's, its
/// install states for those agents dropped, merged with the scripted ones. With none named,
/// `real` and `source` as they are.
fn wrap(
    real: Arc<dyn SetupAgents>,
    source: impl Stream<Item = AgentsEvent> + Send + 'static,
) -> (Arc<dyn SetupAgents>, BoxStream<'static, AgentsEvent>) {
    let installed = agents_from(std::env::var(INSTALLED_VAR).ok().as_deref());
    let missing: BTreeSet<AgentId> = agents_from(std::env::var(AGENTS_VAR).ok().as_deref())
        .difference(&installed)
        .cloned()
        .collect();
    if missing.is_empty() && installed.is_empty() {
        return (real, source.boxed());
    }
    tracing::info!(
        missing = missing.len(),
        installed = installed.len(),
        "e2e: scripted installs stand in for S18's"
    );
    wrap_with(real, source, &missing, &installed, HOLD)
}

/// [`wrap`] for `missing` (each state held `hold`) and `installed`.
fn wrap_with(
    real: Arc<dyn SetupAgents>,
    source: impl Stream<Item = AgentsEvent> + Send + 'static,
    missing: &BTreeSet<AgentId>,
    installed: &BTreeSet<AgentId>,
    hold: Duration,
) -> (Arc<dyn SetupAgents>, BoxStream<'static, AgentsEvent>) {
    let (fake, events) = FakeInstall::new(real, missing, installed, hold);
    let agents: BTreeSet<AgentId> = missing.union(installed).cloned().collect();
    let source = source.filter(move |event| {
        ready(!matches!(event, AgentsEvent::Install { agent, .. } if agents.contains(agent)))
    });
    (Arc::new(fake), merged(source, events))
}

/// What an install's thread shares with the seam: each scripted agent's latest state.
struct Shared {
    events: mpsc::UnboundedSender<AgentsEvent>,
    states: Mutex<BTreeMap<AgentId, InstallState>>,
}

impl Shared {
    /// Says `state` for `agent`, under the lock, so states go out in the order they are said.
    fn say(&self, agent: &AgentId, state: InstallState) {
        self.say_in(&mut lock(&self.states), agent, state);
    }

    /// [`Shared::say`] with the lock held.
    fn say_in(
        &self,
        states: &mut BTreeMap<AgentId, InstallState>,
        agent: &AgentId,
        state: InstallState,
    ) {
        states.insert(agent.clone(), state.clone());
        // Fails only once the pump is gone: nobody listens then.
        drop(self.events.unbounded_send(AgentsEvent::Install {
            agent: agent.clone(),
            state,
        }));
    }
}

/// The steps after the first `Downloading`, each after `hold`.
async fn play(shared: &Shared, agent: &AgentId, hold: Duration) {
    let steps = [
        InstallState::Downloading {
            received: DOWNLOAD_BYTES / 2,
            total: DOWNLOAD_BYTES,
        },
        InstallState::Installing,
        InstallState::Ready {
            version: VERSION.to_owned(),
        },
    ];
    for state in steps {
        async_io::Timer::after(hold).await;
        shared.say(agent, state);
    }
}

/// The seam with scripted installs for some agents.
pub(crate) struct FakeInstall {
    real: Arc<dyn SetupAgents>,
    shared: Arc<Shared>,
    hold: Duration,
}

impl FakeInstall {
    /// The seam over `real` with `agents`' installs scripted, each state held `hold`, and
    /// `installed`'s already done, and the scripts' events: each agent's `Missing` first, each
    /// installed one's `Ready`.
    pub(crate) fn new(
        real: Arc<dyn SetupAgents>,
        agents: &BTreeSet<AgentId>,
        installed: &BTreeSet<AgentId>,
        hold: Duration,
    ) -> (Self, mpsc::UnboundedReceiver<AgentsEvent>) {
        let (events, receiver) = mpsc::unbounded();
        let shared = Arc::new(Shared {
            events,
            states: Mutex::new(BTreeMap::new()),
        });
        for agent in agents {
            let missing = InstallState::Missing {
                download_bytes: DOWNLOAD_BYTES,
            };
            shared.say(agent, missing);
        }
        for agent in installed {
            let ready = InstallState::Ready {
                version: VERSION.to_owned(),
            };
            shared.say(agent, ready);
        }
        let fake = Self { real, shared, hold };
        (fake, receiver)
    }

    /// `agent`'s scripted state, when its install is scripted.
    fn scripted(&self, agent: &AgentId) -> Option<InstallState> {
        lock(&self.shared.states).get(agent).cloned()
    }

    /// Starts `agent`'s install, when it is `Missing`: `Downloading` now (checked and said in
    /// one step under the lock, so two presses start one install), the rest on a thread of its
    /// own. A ready agent says `Ready` again; one playing is joined.
    fn start(&self, agent: &AgentId) {
        {
            let mut states = lock(&self.shared.states);
            match states.get(agent) {
                Some(InstallState::Missing { .. }) => {}
                Some(ready @ InstallState::Ready { .. }) => {
                    let ready = ready.clone();
                    self.shared.say_in(&mut states, agent, ready);
                    return;
                }
                _ => return,
            }
            let downloading = InstallState::Downloading {
                received: 0,
                total: DOWNLOAD_BYTES,
            };
            self.shared.say_in(&mut states, agent, downloading);
        }
        let shared = Arc::clone(&self.shared);
        let who = agent.clone();
        let hold = self.hold;
        let spawned = std::thread::Builder::new()
            .name("clauseo-e2e-install".to_owned())
            .spawn(move || futures::executor::block_on(play(&shared, &who, hold)));
        if let Err(err) = spawned {
            tracing::warn!(%err, "e2e: the scripted install did not start");
            let missing = InstallState::Missing {
                download_bytes: DOWNLOAD_BYTES,
            };
            self.shared.say(agent, missing);
        }
    }
}

impl SetupAgents for FakeInstall {
    fn install(&self, agent: &AgentId) {
        match self.scripted(agent) {
            None => self.real.install(agent),
            Some(_) => self.start(agent),
        }
    }
    fn retry(&self, agent: &AgentId, use_system_proxy: bool) {
        if self.scripted(agent).is_none() {
            self.real.retry(agent, use_system_proxy);
        }
    }
    fn state(&self, agent: &AgentId) -> Option<InstallState> {
        self.scripted(agent).or_else(|| self.real.state(agent))
    }
    fn installed(&self, agent: &AgentId) -> Option<Installed> {
        match self.scripted(agent) {
            Some(_) => None,
            None => self.real.installed(agent),
        }
    }
    fn install_tries(&self, agent: &AgentId) -> Option<u32> {
        match self.scripted(agent) {
            Some(_) => None,
            None => self.real.install_tries(agent),
        }
    }
    fn sign_in(&self, agent: &AgentId, method: Method) -> Result<(), SigninError> {
        self.real.sign_in(agent, method)
    }
    fn cancel_sign_in(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.real.cancel_sign_in(agent)
    }
    fn paste_code(&self, agent: &AgentId, code: PasteCode) -> Result<(), SigninError> {
        self.real.paste_code(agent, code)
    }
    fn sign_out(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.real.sign_out(agent)
    }
    fn check_status(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.real.check_status(agent)
    }
    fn find_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.real.find_login(agent)
    }
    fn use_found_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.real.use_found_login(agent)
    }
    fn use_different_account(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.real.use_different_account(agent)
    }
    fn signin_state(&self, agent: &AgentId) -> Option<SigninState> {
        self.real.signin_state(agent)
    }
    fn sign_in_going(&self, agent: &AgentId) -> bool {
        self.real.sign_in_going(agent)
    }
    fn stop_using_login(&self, agent: &AgentId, why: Option<Found>) -> Result<(), SigninError> {
        self.real.stop_using_login(agent, why)
    }
    fn shutdown(&self, wait: Duration) -> bool {
        self.real.shutdown(wait)
    }
    fn set_network(&self, network: clauseo_agents::Network) {
        self.real.set_network(network);
    }
}

#[cfg(test)]
mod tests;

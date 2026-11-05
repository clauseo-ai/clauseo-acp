//! The fakes the backend's tests run on: `Agents`, the host's calls, the confirms and the
//! opener, each recording what it was asked, and the backend over them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_agents::keys::Keychain;
use clauseo_agents::{Clock, InstallState, Installed, Method, PasteCode, SigninError, SigninState};
use clauseo_protocol::{AgentId, HostError, SetupCommand, SetupEvent};
use clauseo_settings::{AgentLaunch, EntryWrite};
use serde_json::json;

use super::super::{AgentInfo, AgentsBackend, SetupAgents};
use super::{claude, codex};
use crate::bridge::lock;
use crate::dialogs::BoxFuture;
use crate::open::Opener;
use crate::setup::compose::{Compose, is_running};
use crate::setup::confirm::{Ask, SetupConfirm};
use crate::setup::host_calls::SetupHost;
use crate::setup::keys::KeyCache;
use crate::setup::{Setup, SetupBackend, SetupEvents};

/// What the fake `Agents` was asked, by name and agent.
pub(super) type Calls = Arc<Mutex<Vec<(&'static str, AgentId)>>>;

/// `Agents` played by a test: each call recorded; states and installs set by the test.
#[derive(Default)]
pub(super) struct FakeAgents {
    pub(super) calls: Calls,
    pub(super) states: Mutex<BTreeMap<AgentId, InstallState>>,
    pub(super) installs: Mutex<BTreeMap<AgentId, Installed>>,
    pub(super) signin: Mutex<BTreeMap<AgentId, SigninState>>,
    /// What every sign-in call answers instead of `Ok`, once set.
    pub(super) refusal: Mutex<Option<SigninError>>,
    /// The agents whose job is a sign-in, as S18 reserves it: from a call that starts one until
    /// a call that replaces it, so `cancel_sign_in` finds one before any state is reported.
    pub(super) signing_in: Mutex<BTreeSet<AgentId>>,
}

impl FakeAgents {
    fn call(&self, name: &'static str, agent: &AgentId) {
        lock(&self.calls).push((name, agent.clone()));
    }

    fn answer(&self, name: &'static str, agent: &AgentId) -> Result<(), SigninError> {
        self.call(name, agent);
        lock(&self.refusal).clone().map_or(Ok(()), Err)
    }

    /// [`FakeAgents::answer`]; when it is `Ok`, `agent`'s job is now a sign-in or, with
    /// `signs_in` false, a job that replaced it.
    fn reserve(
        &self,
        name: &'static str,
        agent: &AgentId,
        signs_in: bool,
    ) -> Result<(), SigninError> {
        self.answer(name, agent)?;
        let mut signing_in = lock(&self.signing_in);
        if signs_in {
            signing_in.insert(agent.clone());
        } else {
            signing_in.remove(agent);
        }
        Ok(())
    }
}

impl SetupAgents for FakeAgents {
    fn install(&self, agent: &AgentId) {
        self.call("install", agent);
    }
    fn retry(&self, agent: &AgentId, _use_system_proxy: bool) {
        self.call("retry", agent);
    }
    fn state(&self, agent: &AgentId) -> Option<InstallState> {
        lock(&self.states).get(agent).cloned()
    }
    fn installed(&self, agent: &AgentId) -> Option<Installed> {
        lock(&self.installs).get(agent).cloned()
    }
    fn sign_in(&self, agent: &AgentId, method: Method) -> Result<(), SigninError> {
        let name = match method {
            Method::DeviceCode => "sign_in_device_code",
            _ => "sign_in",
        };
        self.reserve(name, agent, true)
    }
    /// S18's: `NotRunning` unless a sign-in job is reserved or its state is a running one.
    fn cancel_sign_in(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.answer("cancel_sign_in", agent)?;
        let running = lock(&self.signin).get(agent).is_some_and(is_running);
        if running || lock(&self.signing_in).contains(agent) {
            Ok(())
        } else {
            Err(SigninError::NotRunning(agent.clone()))
        }
    }
    fn paste_code(&self, agent: &AgentId, _code: PasteCode) -> Result<(), SigninError> {
        self.answer("paste_code", agent)
    }
    fn sign_out(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.reserve("sign_out", agent, false)
    }
    fn check_status(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.answer("check_status", agent)
    }
    fn find_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.reserve("find_login", agent, false)
    }
    fn use_found_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.reserve("use_found_login", agent, true)
    }
    fn use_different_account(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.reserve("use_different_account", agent, true)
    }
    fn signin_state(&self, agent: &AgentId) -> Option<SigninState> {
        lock(&self.signin).get(agent).cloned()
    }
}

/// One host call as `RecordingHost` saw it.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum HostCall {
    SetAgentLaunch(AgentId, AgentLaunch),
    SetEndpoint(AgentId, Option<String>),
    RestartAgents(AgentId),
    StopAgents(AgentId),
    SetReusedLogin(AgentId, bool),
    RemoveLoginLinks(AgentId),
    LoginChanged(AgentId),
}

impl std::fmt::Debug for HostCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetAgentLaunch(agent, launch) => {
                write!(f, "set_agent_launch({agent}, {launch:?})")
            }
            Self::SetEndpoint(agent, url) => write!(f, "set_endpoint({agent}, {url:?})"),
            Self::RestartAgents(agent) => write!(f, "restart_agents({agent})"),
            Self::StopAgents(agent) => write!(f, "stop_agents({agent})"),
            Self::SetReusedLogin(agent, reused) => write!(f, "set_reused_login({agent}, {reused})"),
            Self::RemoveLoginLinks(agent) => write!(f, "remove_login_links({agent})"),
            Self::LoginChanged(agent) => write!(f, "login_changed({agent})"),
        }
    }
}

/// The host's Rust-only calls, recorded in order, each answered `Ok`.
#[derive(Default)]
pub(super) struct RecordingHost(Mutex<Vec<HostCall>>);

impl RecordingHost {
    pub(super) fn calls(&self) -> Vec<HostCall> {
        lock(&self.0).clone()
    }

    fn record(&self, call: HostCall) -> BoxFuture<'_, Result<(), HostError>> {
        lock(&self.0).push(call);
        Box::pin(std::future::ready(Ok(())))
    }
}

impl SetupHost for RecordingHost {
    fn set_agent_launch(
        &self,
        agent: AgentId,
        launch: AgentLaunch,
    ) -> BoxFuture<'_, Result<EntryWrite, HostError>> {
        lock(&self.0).push(HostCall::SetAgentLaunch(agent, launch));
        Box::pin(std::future::ready(Ok(EntryWrite::Written)))
    }
    fn set_endpoint(
        &self,
        agent: AgentId,
        endpoint: Option<String>,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::SetEndpoint(agent, endpoint))
    }
    fn restart_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::RestartAgents(agent))
    }
    fn stop_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::StopAgents(agent))
    }
    fn set_reused_login(
        &self,
        agent: AgentId,
        reused: bool,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::SetReusedLogin(agent, reused))
    }
    fn remove_login_links(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::RemoveLoginLinks(agent))
    }
    fn login_changed(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::LoginChanged(agent))
    }
}

/// The confirms, answered `answer`, each ask recorded.
pub(super) struct FakeConfirm {
    pub(super) answer: bool,
    pub(super) asked: Mutex<Vec<Ask>>,
}

impl SetupConfirm for FakeConfirm {
    fn confirm(&self, ask: Ask) -> BoxFuture<'_, bool> {
        lock(&self.asked).push(ask);
        Box::pin(std::future::ready(self.answer))
    }
}

/// The opener, each URL recorded.
#[derive(Default)]
pub(super) struct FakeOpener(pub(super) Mutex<Vec<String>>);

impl Opener for FakeOpener {
    fn open_url(&self, url: &str) -> Result<(), HostError> {
        lock(&self.0).push(url.to_owned());
        Ok(())
    }
    fn open_path(&self, _path: &Path, _kind: &str) -> Result<(), HostError> {
        Ok(())
    }
    fn reveal(&self, _path: &Path) -> Result<(), HostError> {
        Ok(())
    }
}

pub(super) struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

/// A backend over the fakes, its confirm answering `accept`, and the fakes.
pub(super) struct Rig {
    pub(super) backend: Arc<AgentsBackend>,
    pub(super) agents: Arc<FakeAgents>,
    pub(super) host: Arc<RecordingHost>,
    pub(super) confirm: Arc<FakeConfirm>,
    pub(super) opener: Arc<FakeOpener>,
    pub(super) events: Arc<SetupEvents>,
    /// The memory keychain under the backend's `KeyCache` (clones share it).
    pub(super) keychain: Keychain,
}

pub(super) fn home(agent: &AgentId) -> PathBuf {
    PathBuf::from(format!("/data/agents/{agent}/home"))
}

pub(super) fn rig(accept: bool) -> Rig {
    rig_on(accept, Keychain::memory())
}

/// [`rig`] whose `KeyCache` loads from `keychain` (a memory one) at start.
pub(super) fn rig_on(accept: bool, keychain: Keychain) -> Rig {
    rig_with(accept, keychain, false)
}

/// [`rig`] with Claude's `plans.<id>` set by policy when `plan_locked`.
pub(super) fn rig_locked(plan_locked: bool) -> Rig {
    rig_with(true, Keychain::memory(), plan_locked)
}

fn rig_with(accept: bool, keychain: Keychain, plan_locked: bool) -> Rig {
    let agents = Arc::new(FakeAgents::default());
    let host = Arc::new(RecordingHost::default());
    let confirm = Arc::new(FakeConfirm {
        answer: accept,
        asked: Mutex::default(),
    });
    let opener = Arc::new(FakeOpener::default());
    let events = Arc::new(SetupEvents::default());
    let known = [(claude(), "Claude Code"), (codex(), "Codex")]
        .into_iter()
        .map(|(agent, name)| {
            let info = AgentInfo {
                name: name.to_owned(),
                hosts: vec!["registry.npmjs.org".to_owned()],
                home: home(&agent),
                plan_locked: plan_locked && agent == claude(),
                provider: false,
            };
            (agent, info)
        })
        .collect();
    let backend = Arc::new(AgentsBackend {
        agents: Arc::clone(&agents) as Arc<dyn SetupAgents>,
        keys: Arc::new(KeyCache::load_all(keychain.clone(), [&claude(), &codex()])),
        compose: Arc::new(Compose::new(Arc::clone(&events), Arc::new(Fixed))),
        host: Arc::clone(&host) as Arc<dyn SetupHost>,
        confirm: Arc::clone(&confirm) as Arc<dyn SetupConfirm>,
        opener: Arc::clone(&opener) as Arc<dyn Opener>,
        events: Arc::clone(&events),
        known,
        signin_gate: Mutex::default(),
    });
    Rig {
        backend,
        agents,
        host,
        confirm,
        opener,
        events,
        keychain,
    }
}

impl Rig {
    /// `Setup` over this backend, publishing where the backend does.
    pub(super) fn setup(&self) -> Setup {
        Setup::with_events(
            Arc::clone(&self.backend) as Arc<dyn SetupBackend>,
            Arc::clone(&self.events),
        )
    }

    pub(super) fn calls(&self) -> Vec<(&'static str, AgentId)> {
        lock(&self.agents.calls).clone()
    }
}

/// `command` as a window's JSON decodes, `fields` added to its agent.
pub(super) fn command(name: &str, agent: &AgentId, fields: &serde_json::Value) -> SetupCommand {
    let mut body = json!({ "agent": agent.as_str() });
    body.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    serde_json::from_value(json!({ name: body })).unwrap()
}

pub(super) fn setup_events(
    feed: &mut futures::channel::mpsc::UnboundedReceiver<Arc<str>>,
) -> Vec<SetupEvent> {
    std::iter::from_fn(|| feed.try_recv().ok())
        .map(|text| {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            serde_json::from_value(value["setup"].clone()).unwrap()
        })
        .collect()
}

//! The fakes the backend's tests run on: `Agents`, the host's calls, the confirms and the
//! opener, each recording what it was asked, and the backend over them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_agents::keys::Keychain;
use clauseo_agents::{
    AccountFuture, AgentsEvent, Found, InstallState, Installed, Method, PasteCode, SigninError,
    SigninState,
};
use clauseo_host::{AgentsStopped, LoginSplits};
use clauseo_protocol::{AgentId, AgentKind, ErrorCode, HostError, SetupCommand, SetupEvent};
use clauseo_settings::{AgentLaunch, EntryWrite};
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::stream::BoxStream;
use serde_json::json;

use super::super::{AgentInfo, AgentsBackend, SetupAgents, SetupCounts};
use super::timing::{FakeTimer, Stepping};
use super::{claude, codex};
use crate::bridge::lock;
use crate::dialogs::BoxFuture;
use crate::open::Opener;
use crate::setup::compose::{Compose, is_running};
use crate::setup::confirm::SetupConfirm;
use crate::setup::host_calls::SetupHost;
use crate::setup::identity::Identities;
use crate::setup::keys::KeyCache;
use crate::setup::leave::LeaveTimer;
use crate::setup::login_check::ReusedLogins;
use crate::setup::readers::{LeaveReaders, ReadHold};
use crate::setup::refresh::SettingsRead;
use crate::setup::wiring::Wiring;
use crate::setup::{Setup, SetupBackend, SetupEvents};
use crate::test_support::block_on;

mod small;

pub(super) use self::small::{FakeAccounts, FakeConfirm, FakeCounts, FakeOpener};

/// What the fake `Agents` was asked, by name and agent.
pub(super) type Calls = Arc<Mutex<Vec<(&'static str, AgentId)>>>;

/// Every call to the fake `Agents`, the host and the readers, in one order:
/// `agents.<call>`, `host.<call>` and `readers.<hold|release|set_entry>`.
pub(super) type Journal = Arc<Mutex<Vec<String>>>;

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
    /// Shared with the host and the readers.
    pub(super) journal: Journal,
    /// What S18 reports for a call, by the call's name, sent on `feed` when the call is made.
    pub(super) answers: Mutex<BTreeMap<&'static str, SigninState>>,
    /// Where `answers` go: the pump's source while [`Rig::pumped`] runs.
    pub(super) feed: Mutex<Option<UnboundedSender<AgentsEvent>>>,
    /// What `install_tries` answers, by agent.
    pub(super) tries: Mutex<BTreeMap<AgentId, u32>>,
    /// What `read_account` answers (R17).
    pub(super) accounts: FakeAccounts,
}

impl FakeAgents {
    fn call(&self, name: &'static str, agent: &AgentId) {
        lock(&self.calls).push((name, agent.clone()));
        lock(&self.journal).push(format!("agents.{name}"));
        let answer = lock(&self.answers).get(name).cloned();
        if let (Some(state), Some(feed)) = (answer, &*lock(&self.feed)) {
            lock(&self.signin).insert(agent.clone(), state.clone());
            let event = AgentsEvent::Signin {
                agent: agent.clone(),
                state,
            };
            drop(feed.unbounded_send(event));
        }
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
    fn sign_in_going(&self, agent: &AgentId) -> bool {
        lock(&self.signing_in).contains(agent)
    }
    fn stop_using_login(&self, agent: &AgentId, _why: Option<Found>) -> Result<(), SigninError> {
        self.reserve("stop_using_login", agent, false)
    }
    fn install_tries(&self, agent: &AgentId) -> Option<u32> {
        lock(&self.tries).get(agent).copied()
    }
    fn read_account(&self, agent: &AgentId) -> AccountFuture {
        self.call("read_account", agent);
        self.accounts.read(agent)
    }
    fn shutdown(&self, _wait: Duration) -> bool {
        lock(&self.journal).push("agents.shutdown".to_owned());
        true
    }
}

/// The limits readers' hold and entries, journaled.
pub(super) struct FakeReaders {
    pub(super) journal: Journal,
    pub(super) entries: Mutex<Vec<(AgentKind, Option<AgentLaunch>)>>,
    /// The agents last told blocked, not journaled.
    pub(super) blocked: Mutex<Option<BTreeSet<AgentId>>>,
}

/// A fake hold: journals its release.
struct FakeHold(Journal);

impl Drop for FakeHold {
    fn drop(&mut self) {
        lock(&self.0).push("readers.release".to_owned());
    }
}

impl LeaveReaders for FakeReaders {
    fn hold(&self, _agent: AgentKind) -> BoxFuture<'static, ReadHold> {
        lock(&self.journal).push("readers.hold".to_owned());
        let hold = ReadHold::new(FakeHold(Arc::clone(&self.journal)));
        Box::pin(std::future::ready(hold))
    }
    fn set_entry(&self, agent: AgentKind, launch: Option<AgentLaunch>) {
        lock(&self.journal).push("readers.set_entry".to_owned());
        lock(&self.entries).push((agent, launch));
    }
    fn set_blocked(&self, blocked: BTreeSet<AgentId>) {
        *lock(&self.blocked) = Some(blocked);
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
    SetLaunchRefused(AgentId, Option<ErrorCode>),
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
            Self::SetLaunchRefused(agent, code) => {
                write!(f, "set_launch_refused({agent}, {code:?})")
            }
        }
    }
}

/// The host's Rust-only calls, recorded in order, each answered `Ok`; `stop_agents` answers
/// `stopped` (`Exited` unless a test sets `TimedOut`), and the next `restart_fails`
/// `restart_agents` calls answer `internal`.
pub(super) struct RecordingHost {
    calls: Mutex<Vec<HostCall>>,
    pub(super) journal: Journal,
    pub(super) stopped: Mutex<AgentsStopped>,
    pub(super) restart_fails: Mutex<usize>,
    /// The next `stop_agents` panics, once a test sets it.
    pub(super) stop_panics: Mutex<bool>,
    /// The next `stop_agents` answers once this opens, when a test sets it.
    pub(super) stop_gate: Mutex<Option<futures::channel::oneshot::Receiver<()>>>,
    /// What `settings_changes` answers, once a test sets it.
    pub(super) changes: Mutex<Option<BoxStream<'static, ()>>>,
    /// Told at each `restart_agents`, once a test sets it.
    pub(super) restarted: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

impl RecordingHost {
    pub(super) fn new(journal: Journal) -> Self {
        Self {
            calls: Mutex::default(),
            journal,
            stopped: Mutex::new(AgentsStopped::Exited),
            restart_fails: Mutex::new(0),
            stop_panics: Mutex::new(false),
            stop_gate: Mutex::new(None),
            changes: Mutex::new(None),
            restarted: Mutex::new(None),
        }
    }

    pub(super) fn calls(&self) -> Vec<HostCall> {
        lock(&self.calls).clone()
    }

    fn push(&self, call: HostCall) {
        let name = format!("{call:?}");
        let name = name.split('(').next().unwrap_or_default();
        lock(&self.journal).push(format!("host.{name}"));
        lock(&self.calls).push(call);
    }

    fn record(&self, call: HostCall) -> BoxFuture<'_, Result<(), HostError>> {
        self.push(call);
        Box::pin(std::future::ready(Ok(())))
    }
}

impl SetupHost for RecordingHost {
    fn set_agent_launch(
        &self,
        agent: AgentId,
        launch: AgentLaunch,
    ) -> BoxFuture<'_, Result<EntryWrite, HostError>> {
        self.push(HostCall::SetAgentLaunch(agent, launch));
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
        self.push(HostCall::RestartAgents(agent));
        if let Some(told) = &*lock(&self.restarted) {
            let _told = told.send(());
        }
        let mut fails = lock(&self.restart_fails);
        let answer = if *fails > 0 {
            *fails -= 1;
            Err(HostError::new(ErrorCode::Internal, "the restart failed"))
        } else {
            Ok(())
        };
        Box::pin(std::future::ready(answer))
    }
    fn stop_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<AgentsStopped, HostError>> {
        self.push(HostCall::StopAgents(agent));
        let panics = std::mem::take(&mut *lock(&self.stop_panics));
        assert!(!panics, "the test's stop_agents panics");
        let gate = lock(&self.stop_gate).take();
        let stopped = *lock(&self.stopped);
        Box::pin(async move {
            if let Some(gate) = gate {
                let _opened = gate.await;
            }
            Ok(stopped)
        })
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
    fn set_launch_refused(
        &self,
        agent: AgentId,
        code: Option<ErrorCode>,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::SetLaunchRefused(agent, code))
    }
    fn login_splits(&self) -> BoxFuture<'_, Result<LoginSplits, HostError>> {
        Box::pin(std::future::ready(Err(HostError::new(
            ErrorCode::Internal,
            "the tests feed no splits",
        ))))
    }
    fn settings_changes(&self) -> BoxFuture<'_, Result<BoxStream<'static, ()>, HostError>> {
        lock(&self.journal).push("host.settings_changes".to_owned());
        let answer = lock(&self.changes).take().ok_or_else(|| {
            HostError::new(ErrorCode::Internal, "the tests apply settings directly")
        });
        Box::pin(std::future::ready(answer))
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
    pub(super) readers: Arc<FakeReaders>,
    pub(super) journal: Journal,
    /// The leave's timer, which the test's [`Script`](super::timing::Script) drives.
    pub(super) timer: Arc<FakeTimer>,
    /// The pump's usage counts.
    pub(super) counts: Arc<FakeCounts>,
}

pub(super) fn home(agent: &AgentId) -> PathBuf {
    PathBuf::from(format!("/data/agents/{agent}/home"))
}

pub(super) fn rig(accept: bool) -> Rig {
    rig_on(accept, Keychain::memory())
}

/// [`rig`] whose `KeyCache` loads from `keychain` (a memory one) at start.
pub(super) fn rig_on(accept: bool, keychain: Keychain) -> Rig {
    let keys = KeyCache::load_all(keychain.clone(), [&claude(), &codex()]);
    rig_with(accept, keychain, keys, false)
}

/// [`rig`] over `keys`, loaded from `keychain` (a memory one) with notes, say.
pub(super) fn rig_keys(keychain: Keychain, keys: KeyCache) -> Rig {
    rig_with(true, keychain, keys, false)
}

/// [`rig`] with Claude's `plans.<id>` set by policy when `plan_locked`.
pub(super) fn rig_locked(plan_locked: bool) -> Rig {
    let keychain = Keychain::memory();
    let keys = KeyCache::load_all(keychain.clone(), [&claude(), &codex()]);
    rig_with(true, keychain, keys, plan_locked)
}

fn rig_with(accept: bool, keychain: Keychain, keys: KeyCache, plan_locked: bool) -> Rig {
    rig_full(
        accept,
        keychain,
        keys,
        plan_locked,
        None,
        Identities::in_memory(),
    )
}

/// [`rig`] with each agent's home `<homes>/<id>` and `identity` as its identity check (R17).
pub(super) fn rig_identity(homes: &Path, identity: Identities) -> Rig {
    let keychain = Keychain::memory();
    let keys = KeyCache::load_all(keychain.clone(), [&claude(), &codex()]);
    rig_full(true, keychain, keys, false, Some(homes), identity)
}

fn rig_full(
    accept: bool,
    keychain: Keychain,
    keys: KeyCache,
    plan_locked: bool,
    homes: Option<&Path>,
    identity: Identities,
) -> Rig {
    let journal = Journal::default();
    let agents = Arc::new(FakeAgents {
        journal: Arc::clone(&journal),
        ..FakeAgents::default()
    });
    let host = Arc::new(RecordingHost::new(Arc::clone(&journal)));
    let readers = Arc::new(FakeReaders {
        journal: Arc::clone(&journal),
        entries: Mutex::default(),
        blocked: Mutex::default(),
    });
    let confirm = Arc::new(FakeConfirm {
        answer: accept,
        asked: Mutex::default(),
    });
    let opener = Arc::new(FakeOpener::default());
    let counts = Arc::new(FakeCounts::default());
    let events = Arc::new(SetupEvents::default());
    let clock = Arc::new(Stepping::default());
    let timer = Arc::new(FakeTimer::new(Arc::clone(&clock)));
    let known = [(claude(), "Claude Code"), (codex(), "Codex")]
        .into_iter()
        .map(|(agent, name)| {
            let info = AgentInfo {
                name: name.to_owned(),
                hosts: vec!["registry.npmjs.org".to_owned()],
                home: homes.map_or_else(|| home(&agent), |homes| homes.join(agent.as_str())),
                plan_locked: plan_locked && agent == claude(),
                provider: false,
            };
            (agent, info)
        })
        .collect();
    let backend = Arc::new(AgentsBackend {
        agents: Arc::clone(&agents) as Arc<dyn SetupAgents>,
        keys: Arc::new(keys),
        compose: Arc::new(Compose::new(Arc::clone(&events), clock)),
        host: Arc::clone(&host) as Arc<dyn SetupHost>,
        confirm: Arc::clone(&confirm) as Arc<dyn SetupConfirm>,
        opener: Arc::clone(&opener) as Arc<dyn Opener>,
        events: Arc::clone(&events),
        known,
        signin_gate: Mutex::default(),
        wiring: Wiring::new(
            Arc::clone(&readers) as Arc<dyn LeaveReaders>,
            ReusedLogins::default(),
            Arc::default(),
            SettingsRead::default(),
            Vec::new(),
        )
        .with_timer(Arc::clone(&timer) as Arc<dyn LeaveTimer>)
        .with_identity(identity),
        counts: Arc::clone(&counts) as Arc<dyn SetupCounts>,
    });
    Rig {
        backend,
        agents,
        host,
        confirm,
        opener,
        events,
        keychain,
        readers,
        journal,
        timer,
        counts,
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

    /// Runs `work` while the backend's pump reads what the fake `Agents` answers
    /// (`FakeAgents::answers`), then lets the pump end.
    pub(super) fn pumped<T>(&self, work: impl std::future::Future<Output = T>) -> T {
        let (feed, source) = unbounded();
        *lock(&self.agents.feed) = Some(feed);
        let work = async {
            let out = work.await;
            lock(&self.agents.feed).take();
            out
        };
        let (out, ()) = block_on(futures::future::join(work, self.backend.pump(source)));
        out
    }

    /// [`Rig::pumped`], with the automatic leaves followed beside `work` until it ends.
    pub(super) fn pumped_with_leaves<T>(&self, work: impl std::future::Future<Output = T>) -> T {
        let backend = Arc::clone(&self.backend);
        self.pumped(async move {
            let leaves = std::pin::pin!(crate::setup::leave::follow_auto_leaves(&backend));
            match futures::future::select(std::pin::pin!(work), leaves).await {
                futures::future::Either::Left((out, _)) => out,
                futures::future::Either::Right(((), _)) => panic!("the leaves' queue ended"),
            }
        })
    }

    /// S18's latest state for `agent` is `state`, which the pump has taken already.
    pub(super) fn s18_was(&self, agent: &AgentId, state: SigninState) {
        lock(&self.agents.signin).insert(agent.clone(), state.clone());
        drop(self.backend.wiring.waiters.deliver(agent, &state));
        self.backend.compose.signin(agent, state);
    }

    /// S18 sends `state` for `agent`, as its latest, to the pump [`Rig::pumped`] runs.
    pub(super) fn s18_sends(&self, agent: &AgentId, state: SigninState) {
        lock(&self.agents.signin).insert(agent.clone(), state.clone());
        let feed = lock(&self.agents.feed);
        let feed = feed.as_ref().expect("only while pumped");
        let event = AgentsEvent::Signin {
            agent: agent.clone(),
            state,
        };
        feed.unbounded_send(event).unwrap();
    }

    /// The journal so far, and cleared.
    pub(super) fn journal(&self) -> Vec<String> {
        std::mem::take(&mut *lock(&self.journal))
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

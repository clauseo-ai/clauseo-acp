//! How the app's setup backend starts (R20 P5b piece A). The `setup` command is managed before
//! the window opens, but `Agents` needs the effective settings, which load on the host's start
//! thread, and the keys, which load from the keychain: so the command's backend is a
//! [`BackendCell`] that every command waits on (as the bridge waits on the host's cell), filled
//! by a `clauseo-setup` thread of desktop's own. That thread starts the keys' reads, starts
//! `Agents` with its two launchers and the network route of the settings and the environment
//! (`network.rs`: `network.upstream_proxy`, else `HTTPS_PROXY`; the OS's proxy, which may
//! wait on WPAD, comes from the `clauseo-setup-network` thread it starts, through
//! `Agents::set_network`), publishes `setup_info`, fills the cell, waits for the keys (only the
//! commands that read them wait too, `keys/loaded.rs`), tells `Compose` each agent's key, then
//! runs the pump until `Agents` ends.
//!
//! The two launchers ([`launchers`]): the sign-in launcher is the host's
//! (`host_spawn_options`: the environment cleared, since S18's built environment is a sign-in
//! child's whole environment and `SSH_*`, `NO_BROWSER` and `CLAUDE_CODE_REMOTE` must not reach
//! one); the install launcher inherits the environment, since an install's specs carry only the
//! variables they change and Windows needs `SystemRoot` and `PATH`.
//!
//! Piece B: the host's side ([`HostSide`]) is built on the host's start thread, before the
//! host: the key slot the launch hook and the limits readers read, the readers, the reused
//! logins the login check reads, and the plans snapshot. Once the backend is up a
//! `clauseo-setup-host` thread follows the host's `SettingsChanged` (each one reads the
//! settings again, `refresh.rs`), its login splits, and the automatic leaves (`leave/auto.rs`:
//! a split, or a reuse S18 ended by itself), each on its own.
//!
//! The launch refusals (decision record 0048, ruling 4 and the security review's M3): the host
//! starts with them ([`SetupStart::launch_refusals`], `HostConfig::with_launch_refused`), so no
//! launch comes before them: `signin.policy_unreadable` for each agent an unreadable policy
//! blocks, `agent.keys_loading` for every other known agent. The first load is bounded
//! ([`KEYS_LOAD_BOUND`], on the setup thread's timer); once it is over and the first network
//! route is known (at most until `FIRST_ROUTE_WAIT` after the setup thread's start, the keys'
//! load included, `network/follow.rs`), each agent gets its
//! refusal in force (`setup/refusal.rs`: none, or `agent.key_unreadable` for a stored key
//! that did not read, or a read past the bound of an agent the "key stored" marker marks) on
//! the thread that then follows the settings and the keys' notes, so they never cross; a
//! policy refusal stays until the settings lift it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clauseo_agents::keys::Keychain;
use clauseo_agents::{Agents, AgentsConfig, Clock, Found, Platform, Snapshot, SystemClock};
use clauseo_host::SplitWhy;
use clauseo_process::{AsyncIoTimer, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{AgentId, CommandOutcome, ErrorCode, HostError, SetupCommand, SetupEvent};
use clauseo_settings::{Effective, Locations};
use futures::StreamExt as _;
use futures::channel::oneshot;
use futures::future::{FutureExt as _, Shared, join};

use super::backend::{AgentInfo, AgentsBackend, SetupCounts};
use super::compose::Compose;
use super::confirm::SetupConfirm;
use super::hook::Plans;
use super::host_calls::SetupHost;
use super::keys::{FirstLoad, KEYS_LOAD_BOUND, KeyCache, KeyMarker, KeyNotes, KeySlot, KeysLoaded};
use super::login_check::ReusedLogins;
use super::network::FIRST_ROUTE_WAIT;
use super::network::{
    Follower, NetworkRoute, NetworkSlot, OS_FALLBACK, ProxyEnv, follow, os_lookup, resolve,
};
use super::readers::LeaveReaders;
use super::refresh::SettingsRead;
use super::refusal::{follow_keys, lift_at_start, settle_after_load};
use super::status_timing::TARGET;
use super::wiring::Wiring;
use super::{KeyWrite, SetupBackend, SetupEvents};
use crate::bridge::lock;
use crate::chooser::trace::{at_ms, since};
use crate::dialogs::BoxFuture;
use crate::open::Opener;

/// Where the app's setup backend lands; every command waits for it. The quit closes it
/// ([`BackendCell::shutdown`]): a start that comes after does not start `Agents`.
pub(crate) struct BackendCell {
    state: Mutex<CellState>,
    done: Shared<oneshot::Receiver<()>>,
}

/// What the cell's lock guards: the quit's mark, and the backend once `Agents` have started.
#[derive(Default)]
struct CellState {
    /// The quit has begun: no start after this one starts `Agents`.
    closing: bool,
    backend: Option<Arc<dyn SetupBackend>>,
}

/// The one way to fill a [`BackendCell`]. Dropped unfilled (no thread, no platform, or the
/// quit came first), every command gets `internal`.
pub(crate) struct BackendStart {
    cell: Arc<BackendCell>,
    done: oneshot::Sender<()>,
}

impl BackendCell {
    /// An empty cell, and the start that fills it.
    pub(crate) fn new() -> (Arc<Self>, BackendStart) {
        let (done, wait) = oneshot::channel();
        let cell = Arc::new(Self {
            state: Mutex::default(),
            done: wait.shared(),
        });
        let start = BackendStart {
            cell: Arc::clone(&cell),
            done,
        };
        (cell, start)
    }

    /// The backend, once the cell is filled.
    fn filled(&self) -> Option<Arc<dyn SetupBackend>> {
        lock(&self.state).backend.clone()
    }
}

impl BackendStart {
    /// The setup thread's start step: unless the quit has closed the cell, `start` starts
    /// `Agents` and makes the backend over them, which fills the cell and wakes every command.
    /// All of it runs under the cell's lock, which [`BackendCell::shutdown`] takes too: a quit
    /// either finds the backend and shuts it down, or closes the cell first, and then `start`
    /// does not run and no backend comes (`None`).
    pub(crate) fn start_with<B, T>(self, start: impl FnOnce() -> (Arc<B>, T)) -> Option<(Arc<B>, T)>
    where
        B: SetupBackend + 'static,
    {
        let mut state = lock(&self.cell.state);
        if state.closing {
            return None;
        }
        let (backend, out) = start();
        state.backend = Some(Arc::clone(&backend) as Arc<dyn SetupBackend>);
        drop(state);
        // No receiver left only when the cell itself is gone.
        let _woken = self.done.send(());
        Some((backend, out))
    }
}

impl SetupBackend for BackendCell {
    fn run<'a>(
        &'a self,
        command: SetupCommand,
        key_write: KeyWrite,
        events: &'a SetupEvents,
    ) -> BoxFuture<'a, Result<CommandOutcome, HostError>> {
        Box::pin(async move {
            // Canceled (the start was dropped) leaves the cell empty: `internal` below.
            let _finished = self.done.clone().await;
            match self.filled() {
                Some(backend) => backend.run(command, key_write, events).await,
                None => Err(HostError::new(
                    ErrorCode::Internal,
                    "the agents' setup did not start",
                )),
            }
        })
    }

    fn sign_in_refused(&self, agent: &AgentId) -> Option<HostError> {
        self.filled()?.sign_in_refused(agent)
    }

    /// The quit: the cell closes first, so no start after it starts `Agents`; then the
    /// backend's, if one started (`true` when none did).
    fn shutdown(&self, wait: std::time::Duration) -> bool {
        let backend = {
            let mut state = lock(&self.state);
            state.closing = true;
            state.backend.clone()
        };
        backend.is_none_or(|backend| backend.shutdown(wait))
    }
}

/// The two launchers `Agents` starts with, each made by `make` from its options: the install
/// launcher's (the environment inherited) and the sign-in launcher's (the host's: the
/// environment cleared, `home` shown as `~`).
/// Both end with the app (`SpawnOptions::end_with_parent`, #386): every child of theirs
/// starts on a job thread that outlives it. Never the host's (`host_spawn_options`): its agents
/// start on short-lived prep threads, which would kill them at once.
pub(crate) fn launchers<L>(home: Option<PathBuf>, make: impl Fn(SpawnOptions) -> L) -> (L, L) {
    (
        make(SpawnOptions::new().end_with_parent()),
        make(crate::host_spawn_options(home).end_with_parent()),
    )
}

/// What the setup thread needs from the effective settings, read on the host's start thread.
pub(crate) struct SetupStart {
    data_dir: PathBuf,
    known: BTreeMap<AgentId, AgentInfo>,
    managed: BTreeSet<AgentId>,
    settings: SettingsRead,
    registry: Option<String>,
    bundle: Option<PathBuf>,
}

impl SetupStart {
    /// The refusals the host starts with: `signin.policy_unreadable` for each agent an
    /// unreadable policy blocks, `agent.keys_loading` for every other known agent until the
    /// keys have loaded.
    pub(crate) fn launch_refusals(&self) -> Vec<(AgentId, ErrorCode)> {
        refusals_at_start(self.known.keys(), &self.settings.blocked)
    }

    /// The agents an unreadable policy blocks, as read at start: the limits readers' first.
    pub(crate) fn blocked(&self) -> BTreeSet<AgentId> {
        self.settings.blocked.clone()
    }

    /// The network route at start: the settings' and the environment's, with no OS lookup
    /// (that may wait on WPAD; the setup's network thread makes it, `setup/network.rs`).
    pub(crate) fn network_at_start(&self) -> NetworkRoute {
        resolve(
            &self.settings.network,
            &ProxyEnv::from_process(),
            false,
            &|_| None,
        )
    }

    /// Reads `effective` for the setup thread, bringing the `firm-policy` markers up to date.
    pub(crate) fn read(effective: &Effective, locations: &Locations) -> Self {
        let snapshot = Snapshot::embedded();
        let install = &effective.settings().install;
        let mirror = install
            .registry
            .as_deref()
            .and_then(|registry| url::Url::parse(registry).ok())
            .and_then(|registry| registry.host_str().map(str::to_owned));
        let known: BTreeMap<AgentId, AgentInfo> = snapshot
            .agents()
            .map(|entry| {
                let mut hosts = entry.install_hosts.clone();
                hosts.extend(mirror.clone());
                let info = AgentInfo {
                    name: entry.display_name.clone(),
                    hosts,
                    home: locations.agent_home(entry.id.as_str()),
                    plan_locked: effective.is_locked(&format!("plans.{}", entry.id)),
                    provider: effective
                        .plan(entry.id.as_str())
                        .is_some_and(|plan| plan.provider.is_some()),
                };
                (entry.id.clone(), info)
            })
            .collect();
        let managed = known
            .keys()
            .filter(|agent| effective.is_locked(&format!("agents.{agent}")) || e2e_managed(agent))
            .cloned()
            .collect();
        let settings = SettingsRead::read(effective, known.keys());
        Self {
            data_dir: effective.data_dir().to_path_buf(),
            known,
            managed,
            settings,
            registry: install.registry.clone(),
            bundle: install.bundle.clone(),
        }
    }
}

/// Whether the e2e build's `E2E_AGENTS_MANAGED` names `agent`, whose install setup then takes as
/// the firm's (`Managed`): nothing to install, as the e2e app's agents
/// are `fake_agent`. Always false in any other build, which reads no variable.
pub(super) fn e2e_managed(agent: &AgentId) -> bool {
    #[cfg(feature = "e2e")]
    {
        crate::e2e::agents_managed().contains(agent.as_str())
    }
    #[cfg(not(feature = "e2e"))]
    {
        let _ = agent;
        false
    }
}

/// What the app gives the setup thread besides the settings.
pub(crate) struct SetupParts {
    pub(crate) start: BackendStart,
    pub(crate) events: Arc<SetupEvents>,
    pub(crate) host: Arc<dyn SetupHost>,
    pub(crate) confirm: Arc<dyn SetupConfirm>,
    pub(crate) opener: Arc<dyn Opener>,
    /// The lawyer's home, shown as `~` in a sign-in child's stderr tail.
    pub(crate) home: Option<PathBuf>,
    /// Where the pump's usage counts go: the app's `CountsState`.
    pub(crate) counts: Arc<dyn SetupCounts>,
}

/// What the host's start thread builds for setup before the host starts (piece B).
pub(crate) struct HostSide {
    /// Where the keys land once loaded; the launch hook and the readers read it.
    pub(crate) keys: KeySlot,
    /// The host's limits readers.
    pub(crate) readers: Arc<dyn LeaveReaders>,
    /// The reused logins the host's login check reads.
    pub(crate) reused: ReusedLogins,
    /// The launch hook's plans snapshot.
    pub(crate) plans: Plans,
    /// The launch hook's and the readers' network route, from [`SetupStart::network_at_start`]
    /// until the setup's network thread replaces it.
    pub(crate) network: NetworkSlot,
    /// Reads the effective settings again, on each `SettingsChanged`.
    pub(crate) reread: Box<dyn Fn() -> Effective + Send>,
    /// The keys' notes; the host was given its retry hook.
    pub(crate) notes: Arc<KeyNotes>,
}

/// Starts the `clauseo-setup` thread: keys, `Agents`, the first states, the cell, the pump.
pub(crate) fn spawn(read: SetupStart, parts: SetupParts, side: HostSide) -> std::io::Result<()> {
    let thread = std::thread::Builder::new()
        .name("clauseo-setup".to_owned())
        .spawn(move || run(read, parts, side))?;
    // Not joined: it runs as long as `Agents` does.
    drop(thread);
    Ok(())
}

/// The setup thread's work.
fn run(read: SetupStart, parts: SetupParts, side: HostSide) {
    // The first route's wait counts from here (review L5). The OS's proxy lookups start now, on
    // the network thread, beside the keys' load and off the window's start path.
    let setup_start = SystemClock.monotonic();
    let network = follow(
        Follower::unbound(side.network, ProxyEnv::from_process, os_lookup, OS_FALLBACK),
        setup_start,
    );
    network.update(read.settings.network.clone());
    let marker = KeyMarker::new(&read.data_dir, read.known.keys());
    // The keys' reads start now and are waited for once the cell is filled, so a command that
    // reads no key never waits on the keychain (`keys/loaded.rs`).
    let load_began = crate::open::now();
    tracing::info!(target: TARGET, at_ms = at_ms(), "keychain load started");
    let notes = side.notes.sender();
    let (keys, load) = KeyCache::start_load(Keychain::os(), read.known.keys(), marker, notes);
    let keys = Arc::new(keys);
    let (keys_loaded, load_done) = KeysLoaded::pending();
    let refused_at_start = read.launch_refusals();
    let Some(platform) = Platform::current() else {
        tracing::warn!("no agent installs on this platform: setup answers internal");
        drop(parts.start);
        wait_for_keys(load, &keys, load_began, &side.keys);
        let blocked = &read.settings.blocked;
        let settle = settle_after_load(&*parts.host, &keys, read.known.keys(), blocked);
        futures::executor::block_on(settle);
        return;
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    // The settings' and the environment's route; the OS's follows from the network thread.
    let at_start = read.network_at_start().agents_network();
    let mut config = AgentsConfig::new(read.data_dir, platform);
    config.signin.homes = read
        .known
        .iter()
        .map(|(agent, info)| (agent.clone(), info.home.clone()))
        .collect();
    config.managed = read.managed;
    config.install.registry = read.registry;
    config.install.bundle = read.bundle;
    config.install.proxy = at_start.proxy;
    config.install.signin_proxy = at_start.signin_proxy;
    config.install.ca_certs = at_start.ca_certs;
    // Each install try asks for a fresh route first (one past 60 s is resolved again).
    config.install.refresh = Some(Arc::new(network.refresher()));
    config.clock = Arc::clone(&clock);
    // The quit may close the cell first: then `Agents` never starts (`BackendStart::start_with`).
    let started = parts.start.start_with(|| {
        let (install, signin) = launchers(parts.home, |options| {
            Arc::new(PassthroughLauncher::new(options)) as Arc<dyn Launcher>
        });
        let (agents, source) = Agents::start_with_launchers(config, install, signin, AsyncIoTimer);
        let agents: Arc<dyn super::backend::SetupAgents> = Arc::new(agents);
        // The e2e build's scripted installs (`E2E_FAKE_INSTALL`) and sign-ins
        // (`E2E_FAKE_SIGNIN`); no other build has them.
        #[cfg(feature = "e2e")]
        let (agents, source) = super::fake_install::scripted(agents, source, Arc::clone(&clock));
        parts.events.publish(SetupEvent::SetupInfo {
            keys_available: keys.available(),
        });
        let compose = Arc::new(Compose::new(Arc::clone(&parts.events), clock));
        let facts = &read.settings;
        for agent in read.known.keys() {
            compose.set_blocked(agent, facts.blocked.contains(agent));
            compose.set_firm(agent, facts.firm.contains(agent));
        }
        let firm_policy = facts.firm_policy.clone();
        // The route resolved so far, if any, goes first in `Agents`' queue.
        network.bind(Arc::clone(&agents));
        let wiring = Wiring::new(
            side.readers,
            side.reused,
            side.plans,
            read.settings,
            refused_at_start,
        )
        .with_network(network);
        let backend = Arc::new(AgentsBackend {
            agents,
            keys: Arc::clone(&keys),
            compose,
            host: parts.host,
            confirm: parts.confirm,
            opener: parts.opener,
            events: parts.events,
            known: read.known,
            signin_gate: Mutex::new(()),
            rechecks: Mutex::default(),
            pending: Mutex::default(),
            wiring,
            counts: parts.counts,
            keys_loaded,
        });
        for agent in backend.known.keys() {
            backend.set_firm_policy(agent, firm_policy.contains(agent));
        }
        (backend, source)
    });
    // Only the commands that read the keys wait for this (`AgentsBackend::answer`).
    wait_for_keys(load, &keys, load_began, &side.keys);
    let Some((backend, source)) = started else {
        tracing::info!("the quit came first: no agents start");
        return;
    };
    for agent in backend.known.keys() {
        backend.compose.show_key(agent, &keys);
    }
    load_done.finish();
    let follower = Arc::clone(&backend);
    let reread = side.reread;
    let notes = side.notes.take();
    let host_thread = std::thread::Builder::new()
        .name("clauseo-setup-host".to_owned())
        .spawn(move || {
            futures::executor::block_on(lift_at_start(&follower, FIRST_ROUTE_WAIT));
            let settings = follow_settings(&follower, &*reread);
            let leaves = super::leave::follow_auto_leaves(&follower);
            let keys = async {
                if let Some(notes) = notes {
                    follow_keys(&follower, notes).await;
                }
            };
            let splits = join(follow_splits(&follower), follow_sign_in_holds(&follower));
            let follows = join(join(settings, keys), join(splits, leaves));
            futures::executor::block_on(follows);
        });
    if let Err(err) = host_thread {
        tracing::warn!(%err, "no thread follows the host: settings and splits are not followed");
        futures::executor::block_on(lift_at_start(&backend, FIRST_ROUTE_WAIT));
    }
    futures::executor::block_on(backend.pump(source));
}

/// Waits for the keys' first load, at most [`KEYS_LOAD_BOUND`], and logs how long it took and
/// how it ended (no key, no agent's name); then the launch hook and the readers get the keys.
fn wait_for_keys(load: FirstLoad, keys: &Arc<KeyCache>, began: Instant, slot: &KeySlot) {
    let outcome = load.wait(keys, &AsyncIoTimer, KEYS_LOAD_BOUND);
    tracing::info!(target: TARGET, at_ms = at_ms(), took_ms = since(began),
        outcome = outcome.as_str(), "keychain load ended");
    // Set once, here: nothing else fills the slot.
    let _set = slot.set(Arc::clone(keys));
}

/// Each of `known` with the refusal the host starts with: `signin.policy_unreadable` when in
/// `blocked`, else `agent.keys_loading`.
pub(crate) fn refusals_at_start<'a>(
    known: impl IntoIterator<Item = &'a AgentId>,
    blocked: &BTreeSet<AgentId>,
) -> Vec<(AgentId, ErrorCode)> {
    known
        .into_iter()
        .map(|agent| {
            let code = if blocked.contains(agent) {
                ErrorCode::SigninPolicyUnreadable
            } else {
                ErrorCode::AgentKeysLoading
            };
            (agent.clone(), code)
        })
        .collect()
}

/// Each `SettingsChanged`: the settings read again and taken in, until the host stops. Once
/// subscribed they are read again at once, since a change between [`SetupStart::read`] and the
/// subscription sends no `SettingsChanged` this sees (review N2).
pub(crate) async fn follow_settings(
    backend: &AgentsBackend,
    reread: &(dyn Fn() -> Effective + Send),
) {
    let mut changes = match backend.host.settings_changes().await {
        Ok(changes) => changes,
        Err(error) => {
            tracing::warn!(
                code = error.code().as_str(),
                "settings changes not followed"
            );
            crate::ui_language::follow(reread().settings().ui.language);
            return;
        }
    };
    loop {
        let effective = reread();
        // The installer and uninstaller read the language from the registry (ui_language.rs).
        crate::ui_language::follow(effective.settings().ui.language);
        let read = SettingsRead::read(&effective, backend.known.keys());
        super::refresh::apply_settings(backend, read).await;
        if changes.next().await.is_none() {
            break;
        }
    }
}

/// Each login split the host's check finds: an automatic leave, with S18's reason, until the
/// host stops (`leave/auto.rs`: it tries again until it finishes).
async fn follow_splits(backend: &AgentsBackend) {
    let splits = match backend.host.login_splits().await {
        Ok(splits) => splits,
        Err(error) => {
            tracing::warn!(code = error.code().as_str(), "login splits not followed");
            return;
        }
    };
    while let Ok(split) = splits.recv().await {
        backend
            .wiring
            .leaves
            .submit(&split.agent, Some(leave_reason(split.why)));
    }
}

/// S18's reason for a split's automatic leave, which its `SignedOut` shows. `CodexSandbox`'s
/// leave writes `no-reuse` too (`clauseo_agents`), so detection does not offer that login again.
pub(crate) const fn leave_reason(why: SplitWhy) -> Found {
    match why {
        SplitWhy::LinkSplit => Found::CodexLinkSplit,
        SplitWhy::LawyersFileGone => Found::ReusedSignedOut,
        SplitWhy::CodexSandbox => Found::CodexSandbox,
    }
}

/// Each queue newly held for an agent's sign-in, which a launch or a turn failing with
/// `agent.auth_required` leaves: that agent's status checked again
/// ([`AgentsBackend::recheck_sign_in`]), so the sign-in line never stays "signed in" under the
/// error. Until the host stops.
pub(crate) async fn follow_sign_in_holds(backend: &AgentsBackend) {
    let mut holds = match backend.host.sign_in_holds().await {
        Ok(holds) => holds,
        Err(error) => {
            tracing::warn!(code = error.code().as_str(), "sign-in holds not followed");
            return;
        }
    };
    while let Some(agent) = holds.next().await {
        backend.recheck_sign_in(&agent);
    }
}

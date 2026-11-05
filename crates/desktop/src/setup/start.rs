//! How the app's setup backend starts (R20 P5b piece A). The `setup` command is managed before
//! the window opens, but `Agents` needs the effective settings, which load on the host's start
//! thread, and the keys, which load from the keychain: so the command's backend is a
//! [`BackendCell`] that every command waits on (as the bridge waits on the host's cell), filled
//! by a `clauseo-setup` thread of desktop's own. That thread loads the keys, starts `Agents`
//! with its two launchers, publishes `setup_info` and each agent's composed sign-in, fills the
//! cell, then runs the pump until `Agents` ends.
//!
//! The two launchers ([`launchers`]): the sign-in launcher is the host's
//! (`host_spawn_options`: the environment cleared, since S18's built environment is a sign-in
//! child's whole environment and `SSH_*`, `NO_BROWSER` and `CLAUDE_CODE_REMOTE` must not reach
//! one); the install launcher inherits the environment, since an install's specs carry only the
//! variables they change and Windows needs `SystemRoot` and `PATH`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use clauseo_agents::keys::Keychain;
use clauseo_agents::{Agents, AgentsConfig, Clock, Platform, Snapshot, SystemClock};
use clauseo_process::{AsyncIoTimer, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{AgentId, CommandOutcome, ErrorCode, HostError, SetupCommand, SetupEvent};
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::channel::oneshot;
use futures::future::{FutureExt as _, Shared};

use super::backend::{AgentInfo, AgentsBackend};
use super::compose::Compose;
use super::confirm::SetupConfirm;
use super::firm_policy::firm_policy;
use super::host_calls::SetupHost;
use super::keys::KeyCache;
use super::{KeyWrite, SetupBackend, SetupEvents};
use crate::dialogs::BoxFuture;
use crate::open::Opener;

/// Where the app's setup backend lands; every command waits for it.
pub(crate) struct BackendCell {
    backend: OnceLock<Arc<dyn SetupBackend>>,
    done: Shared<oneshot::Receiver<()>>,
}

/// The one way to fill a [`BackendCell`]. Dropped unfilled (no thread, no platform), every
/// command gets `internal`.
pub(crate) struct BackendStart {
    cell: Arc<BackendCell>,
    done: oneshot::Sender<()>,
}

impl BackendCell {
    /// An empty cell, and the start that fills it.
    pub(crate) fn new() -> (Arc<Self>, BackendStart) {
        let (done, wait) = oneshot::channel();
        let cell = Arc::new(Self {
            backend: OnceLock::new(),
            done: wait.shared(),
        });
        let start = BackendStart {
            cell: Arc::clone(&cell),
            done,
        };
        (cell, start)
    }
}

impl BackendStart {
    /// Puts `backend` in the cell and wakes every waiting command.
    pub(crate) fn finish(self, backend: Arc<dyn SetupBackend>) {
        // The only setter, and `self` is consumed: the cell is empty here.
        let _set = self.cell.backend.set(backend);
        // No receiver left only when the cell itself is gone.
        let _woken = self.done.send(());
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
            match self.backend.get() {
                Some(backend) => backend.run(command, key_write, events).await,
                None => Err(HostError::new(
                    ErrorCode::Internal,
                    "the agents' setup did not start",
                )),
            }
        })
    }
}

/// The two launchers `Agents` starts with, each made by `make` from its options: the install
/// launcher's (the environment inherited) and the sign-in launcher's (the host's: the
/// environment cleared, `home` shown as `~`).
pub(crate) fn launchers<L>(home: Option<PathBuf>, make: impl Fn(SpawnOptions) -> L) -> (L, L) {
    (
        make(SpawnOptions::new()),
        make(crate::host_spawn_options(home)),
    )
}

/// What the setup thread needs from the effective settings, read on the host's start thread.
pub(crate) struct SetupStart {
    data_dir: PathBuf,
    known: BTreeMap<AgentId, AgentInfo>,
    managed: BTreeSet<AgentId>,
    firm: BTreeSet<AgentId>,
    blocked: BTreeSet<AgentId>,
    registry: Option<String>,
    bundle: Option<PathBuf>,
}

impl SetupStart {
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
            .filter(|agent| effective.is_locked(&format!("agents.{agent}")))
            .cloned()
            .collect();
        let firm = known
            .keys()
            .filter(|agent| {
                effective
                    .plan(agent.as_str())
                    .is_some_and(|plan| plan.endpoint.is_some() || plan.provider.is_some())
            })
            .cloned()
            .collect();
        let blocked = firm_policy(effective, &SystemFiles, known.keys());
        Self {
            data_dir: effective.data_dir().to_path_buf(),
            known,
            managed,
            firm,
            blocked,
            registry: install.registry.clone(),
            bundle: install.bundle.clone(),
        }
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
}

/// Starts the `clauseo-setup` thread: keys, `Agents`, the first states, the cell, the pump.
pub(crate) fn spawn(read: SetupStart, parts: SetupParts) -> std::io::Result<()> {
    let thread = std::thread::Builder::new()
        .name("clauseo-setup".to_owned())
        .spawn(move || run(read, parts))?;
    // Not joined: it runs as long as `Agents` does.
    drop(thread);
    Ok(())
}

/// The setup thread's work.
fn run(read: SetupStart, parts: SetupParts) {
    let Some(platform) = Platform::current() else {
        tracing::warn!("no agent installs on this platform: setup answers internal");
        return;
    };
    let keys = Arc::new(KeyCache::load_all(Keychain::os(), read.known.keys()));
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let mut config = AgentsConfig::new(read.data_dir, platform);
    config.signin.homes = read
        .known
        .iter()
        .map(|(agent, info)| (agent.clone(), info.home.clone()))
        .collect();
    config.managed = read.managed;
    config.install.registry = read.registry;
    config.install.bundle = read.bundle;
    config.install.proxy = env_proxy();
    config.clock = Arc::clone(&clock);
    let (install, signin) = launchers(parts.home, |options| {
        Arc::new(PassthroughLauncher::new(options)) as Arc<dyn Launcher>
    });
    let (agents, source) = Agents::start_with_launchers(config, install, signin, AsyncIoTimer);
    parts.events.publish(SetupEvent::SetupInfo {
        keys_available: keys.available(),
    });
    let compose = Arc::new(Compose::new(Arc::clone(&parts.events), clock));
    for agent in read.known.keys() {
        compose.set_blocked(agent, read.blocked.contains(agent));
        compose.set_firm(agent, read.firm.contains(agent));
        compose.set_key(agent, keys.has(agent));
    }
    let backend = Arc::new(AgentsBackend {
        agents: Arc::new(agents),
        keys,
        compose,
        host: parts.host,
        confirm: parts.confirm,
        opener: parts.opener,
        events: parts.events,
        known: read.known,
    });
    parts
        .start
        .finish(Arc::clone(&backend) as Arc<dyn SetupBackend>);
    futures::executor::block_on(backend.pump(source));
}

/// The install's proxy from the environment: `HTTPS_PROXY`, else `https_proxy`, when set and
/// not empty. (R17's `network.upstream_proxy` takes its place once it exists.)
fn env_proxy() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
}

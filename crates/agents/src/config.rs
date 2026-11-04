//! What [`crate::Agents`] is started with: the data folder, the snapshot, the platform, the firm's
//! install settings, the ids policy manages, where to look for the user's Node, what sign-in
//! runs are built from, and a clock.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use clauseo_protocol::AgentId;

use crate::platform::Platform;
use crate::snapshot::Snapshot;

/// Everything an `Agents` needs, read once at start.
#[derive(Clone)]
#[non_exhaustive]
pub struct AgentsConfig {
    /// `Locations::data_dir`: every folder this crate writes is under `<data_dir>/agents`.
    pub data_dir: PathBuf,
    /// The agents and Node this build installs: [`Snapshot::embedded`] outside tests.
    pub snapshot: Snapshot,
    /// The platform packages and Node are chosen for: [`Platform::current`] outside tests.
    pub platform: Platform,
    /// The firm's install settings.
    pub install: InstallConfig,
    /// The ids whose `agents.<id>` entry policy sets: IT's runtime. Nothing is installed for
    /// them, and they report `Managed`.
    pub managed: BTreeSet<AgentId>,
    /// Where to look for a Node the user already has.
    pub node_search: NodeSearch,
    /// What sign-in, status and detection runs are built from: each agent's home and the base
    /// environment.
    pub signin: SigninConfig,
    /// The receipt's install time and the progress throttle's clock; sign-in's `since` and `at`.
    pub clock: Arc<dyn Clock>,
    /// Tests only: agents taken as `Ready` at start with this install, with no receipt or Node
    /// check, so the sign-in tests can run a fake CLI as the adapter. Outside `test-support`
    /// the field does not exist.
    #[cfg(feature = "test-support")]
    pub ready_for_tests: BTreeMap<AgentId, crate::state::Installed>,
}

impl AgentsConfig {
    /// The embedded snapshot for `platform`, with no firm settings, nothing managed, the user's
    /// Node searched for as [`NodeSearch::from_env`] finds it, sign-in as
    /// [`SigninConfig::from_env`] (no agent homes yet: R20 sets them), and the system clock.
    pub fn new(data_dir: PathBuf, platform: Platform) -> AgentsConfig {
        AgentsConfig {
            data_dir,
            snapshot: Snapshot::embedded(),
            platform,
            install: InstallConfig::default(),
            managed: BTreeSet::new(),
            node_search: NodeSearch::from_env(),
            signin: SigninConfig::from_env(),
            clock: Arc::new(SystemClock),
            #[cfg(feature = "test-support")]
            ready_for_tests: BTreeMap::new(),
        }
    }
}

impl std::fmt::Debug for AgentsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentsConfig")
            .field("data_dir", &self.data_dir)
            .field("platform", &self.platform)
            .field("install", &self.install)
            .field("managed", &self.managed)
            .field("node_search", &self.node_search)
            .field("signin", &self.signin)
            .finish_non_exhaustive()
    }
}

/// A firm's install settings (R20's `install.*` keys and R17's `network.*` keys). With none
/// set, packages come from the snapshot's registry and Node from its download host, through
/// `HTTPS_PROXY` when the environment sets it.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct InstallConfig {
    /// `install.registry`: a firm npm mirror. Each tarball URL's registry prefix is replaced by
    /// this base before the download; the lock's integrity check is unchanged.
    pub registry: Option<String>,
    /// `install.bundle`: an offline bundle folder (`node/<archive>`, `npm/<sha512 hex>.tgz`),
    /// read instead of the network, with the same checks.
    pub bundle: Option<PathBuf>,
    /// The proxy: `network.upstream_proxy`, else `HTTPS_PROXY` from the environment (the caller
    /// resolves which), else none.
    pub proxy: Option<String>,
    /// `network.ca_certs`: an agent's extra CA file (PEM), the TLS roots of its downloads. npm
    /// never gets it, since npm makes no network call here.
    pub ca_certs: BTreeMap<AgentId, PathBuf>,
    /// Tests only: resolves host names instead of the OS, so a test can be offline and come
    /// back, and can see every host a download looked up.
    #[cfg(feature = "test-support")]
    pub resolver: Option<crate::TestResolver>,
    /// Tests only: lets downloads and the mirror use plain `http://`, for the loopback test
    /// registry. Outside `test-support` the field does not exist, so every download is https.
    #[cfg(feature = "test-support")]
    pub allow_http_for_tests: bool,
}

/// What S18's runs are built from. Proxy and CA come from [`InstallConfig::proxy`] and
/// [`InstallConfig::ca_certs`]: the same `network.*` keys.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct SigninConfig {
    /// Each agent's home folder, `<data_dir>/agents/<id>/home`: the text
    /// `Locations::agent_home(id)` gives (`settings`), passed in by the caller and never
    /// canonicalised. Claude's login store is keyed by this text (`CLAUDE_CONFIG_DIR`, and later
    /// `CLAUDE_SECURESTORAGE_CONFIG_DIR` with the same text), so a differently spelt path is a
    /// different login. This crate does not depend on `settings` and never computes it; an agent
    /// with no entry has no sign-in of Clauseo's own.
    pub homes: BTreeMap<AgentId, PathBuf>,
    /// The environment each run's is built from: this process's (`std::env::vars_os`) outside
    /// tests. Every run drops `SSH_*`, `NO_BROWSER`, `CLAUDE_CODE_REMOTE` and the agents' home
    /// variables from it and adds its own; the child gets nothing else, so the caller's launcher
    /// must clear the inherited environment (`SpawnOptions::clear_env`). A test passes its own
    /// map here instead of changing its process's environment. `Debug` prints only how many
    /// names it holds: values can be secrets.
    pub base_env: BTreeMap<OsString, OsString>,
    /// Tests only: the port Codex's browser sign-in probes in place of 1455, so tests that hold
    /// "the" callback port run side by side. `Some(0)` is a port that is always free. Outside
    /// `test-support` the field does not exist, so the probe is always `127.0.0.1:1455`.
    #[cfg(feature = "test-support")]
    pub callback_port: Option<u16>,
}

/// The port Codex's browser sign-in listens on for its callback: `http://127.0.0.1:1455`.
pub(crate) const CODEX_CALLBACK_PORT: u16 = 1455;

impl SigninConfig {
    /// This process's environment as the base, and no agent homes.
    pub fn from_env() -> SigninConfig {
        SigninConfig {
            homes: BTreeMap::new(),
            base_env: std::env::vars_os().collect(),
            #[cfg(feature = "test-support")]
            callback_port: None,
        }
    }

    /// The lawyer's own home folder, where Codex finds `~/.codex` and Claude Code `~/.claude`:
    /// `base_env`'s `HOME` (`USERPROFILE` on Windows), when set and not empty. R20 and R17 pass
    /// it to [`crate::codex_link_holds`] and [`crate::codex_login_shared`].
    pub fn lawyer_home(&self) -> Option<PathBuf> {
        let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        self.base_env
            .iter()
            .find(|(key, _)| {
                if cfg!(windows) {
                    key.to_string_lossy().eq_ignore_ascii_case(name)
                } else {
                    key.as_os_str() == name
                }
            })
            .map(|(_, value)| PathBuf::from(value))
            .filter(|path| !path.as_os_str().is_empty())
    }

    /// The port Codex's browser sign-in needs free: 1455, or a test's own.
    pub(crate) fn callback_port(&self) -> u16 {
        #[cfg(feature = "test-support")]
        if let Some(port) = self.callback_port {
            return port;
        }
        CODEX_CALLBACK_PORT
    }
}

impl std::fmt::Debug for SigninConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigninConfig")
            .field("homes", &self.homes)
            .field("base_env", &format_args!("{} names", self.base_env.len()))
            .finish_non_exhaustive()
    }
}

/// Where to look for a Node the user already has: each folder of `path` (the `PATH` value),
/// and on Windows `<program_files>\nodejs`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct NodeSearch {
    /// The `PATH` value to search, or `None` to search nothing on it.
    pub path: Option<OsString>,
    /// `%ProgramFiles%` (Windows), or `None`.
    pub program_files: Option<PathBuf>,
}

impl NodeSearch {
    /// `PATH` and `ProgramFiles` from this process's environment.
    pub fn from_env() -> NodeSearch {
        NodeSearch {
            path: std::env::var_os("PATH"),
            program_files: std::env::var_os("ProgramFiles").map(PathBuf::from),
        }
    }

    /// Searches nothing: the user's Node is never found, and Node is always downloaded.
    pub fn none() -> NodeSearch {
        NodeSearch::default()
    }
}

/// The time sources the crate reads. `std`'s clocks are banned in the workspace
/// (`clippy.toml`); [`SystemClock`] is the one place this crate reads them.
pub trait Clock: Send + Sync {
    /// The wall-clock time, for the receipt's `installed_at`.
    fn now(&self) -> SystemTime;
    /// A monotonic reading, for the 250 ms progress throttle. Only differences matter.
    fn monotonic(&self) -> Duration;
}

/// The real clocks.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    #[expect(
        clippy::disallowed_methods,
        reason = "the receipt's install time; agents reads the wall clock here alone"
    )]
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the progress throttle; agents reads the monotonic clock here alone"
    )]
    fn monotonic(&self) -> Duration {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        let start = *START.get_or_init(Instant::now);
        Instant::now().saturating_duration_since(start)
    }
}

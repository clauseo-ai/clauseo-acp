//! What `Host::start` needs: the settings, the launcher, and the parts tests swap out.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_acp::McpServer;
use clauseo_limits::ReadLimits;
use clauseo_process::Launcher;
use clauseo_profile::SkillWrite;
use clauseo_protocol::{AgentId, MatterId, ThreadId};
use clauseo_settings::{Effective, PlatformDirs as _, SystemDirs};
use serde_json::{Map, Value};

use crate::clock::{Clock, SystemClock};
use crate::ids::{IdSource, RandomV7};

use crate::handle::SUBSCRIPTION_CAPACITY;
use crate::window::PATCH_WINDOW_BYTES;

/// What Clauseo's launch profile adds to an agent's session and launch (decision record 0030).
/// The host calls it at each launch and session start, with only the agent's id, then fills the
/// per-session placeholders (`clauseo_profile::placeholders`) and sets its own keys over what
/// the hook returned (`agent/loads.rs` lists them):
///
/// - `session_meta` becomes the `_meta` of `session/new` and `session/load`. A
///   `claudeCode.options.mcpServers` in it is removed in every MCP control mode: servers come
///   only from [`ProfileHook::mcp_servers`].
/// - `launch_env` fills the hook slot of the agent's launch.
/// - `mcp_servers` go after the lawyer's enabled servers. The host fills each `browser`
///   entry's tokens per session, or drops the entry (no proxy, no browser, too many browsers,
///   `tools.browser` off, or the firm's `managed-mcp.json`).
/// - `codex_config` keys go into a Codex agent's `CODEX_CONFIG`, added only where R15's keys
///   and the host's own (`web_search`, `hooks`) leave room.
/// - `skill_files` are written on the launch-prep thread before the agent starts.
///
/// Every method runs on the host thread, at each launch and session start, and must do no I/O:
/// no file, network or process call, and nothing that blocks. It hands back data the adapter
/// already holds; the host does any file work (writing `skill_files`) off that thread.
pub trait ProfileHook: Send {
    /// The `_meta` object for `session/new` and `session/load` with `agent`, or `None` for none.
    fn session_meta(&self, agent: &AgentId) -> Option<Map<String, Value>>;
    /// Variables set on top of the agent's own when it launches.
    fn launch_env(&self, agent: &AgentId) -> BTreeMap<OsString, OsString>;
    /// MCP servers for `agent`'s sessions, after the lawyer's (default: none). Strings may hold
    /// `clauseo_profile::placeholders::IN_SERVERS` tokens.
    fn mcp_servers(&self, _agent: &AgentId) -> Vec<McpServer> {
        Vec::new()
    }
    /// Keys for a Codex agent's `CODEX_CONFIG`, as nested objects (default: none). Strings may
    /// hold `clauseo_profile::placeholders::IN_INSTRUCTIONS` tokens in `developer_instructions`.
    fn codex_config(&self, _agent: &AgentId) -> Map<String, Value> {
        Map::new()
    }
    /// Skill files to write before `agent` starts (default: none). The desktop adapter hands
    /// Claude's plugin skills for a Codex agent too, which Codex reads through the index in its
    /// instructions. A write failure is a warning, never a failed launch.
    fn skill_files(&self, _agent: &AgentId) -> Option<SkillWrite> {
        None
    }
}

/// One agent session, as the egress proxy knows it: the matter, the thread, and the launch (the
/// launch prep's id, from the same counter as the agent's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionKey {
    /// The thread's matter.
    pub matter: MatterId,
    /// The thread.
    pub thread: ThreadId,
    /// The launch.
    pub launch: u64,
}

impl SessionKey {
    /// The session's id, `<thread>-<launch>`: the name of its `run/` folder and agent-browser's
    /// session name.
    pub fn id(&self) -> String {
        format!("{}-{}", self.thread, self.launch)
    }
}

/// The egress proxy, as the host asks it (stage C's R17 implements it). Until it exists,
/// production passes none and the `browser` entry is left out (`NoProxy`).
pub trait EgressProxy: Send + Sync {
    /// The port of `session`'s browser listener, or none.
    fn port_for(&self, session: SessionKey) -> Option<u16>;
}

/// A proxy stand-in for tests: the same port for every session.
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, Copy)]
pub struct TestProxy(pub u16);

#[cfg(feature = "test-support")]
impl EgressProxy for TestProxy {
    fn port_for(&self, _session: SessionKey) -> Option<u16> {
        Some(self.0)
    }
}

/// What the browser entry and the agents' `PATH` need beyond the hook: the proxy, stetdoc's
/// folder, and (until S27's host PR) the Chrome and the slot count as plain inputs.
#[derive(Clone)]
pub(crate) struct LaunchWiring {
    pub(crate) proxy: Option<Arc<dyn EgressProxy>>,
    pub(crate) path_prepend: Option<PathBuf>,
    pub(crate) browser_executable: Option<PathBuf>,
    /// Browsers allowed at once, and browsers open now.
    pub(crate) browser_slots: (usize, usize),
}

impl Default for LaunchWiring {
    fn default() -> Self {
        Self {
            proxy: None,
            path_prepend: None,
            browser_executable: None,
            browser_slots: (usize::MAX, 0),
        }
    }
}

impl std::fmt::Debug for LaunchWiring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchWiring")
            .field("proxy", &self.proxy.is_some())
            .field("path_prepend", &self.path_prepend)
            .field("browser_executable", &self.browser_executable)
            .field("browser_slots", &self.browser_slots)
            .finish()
    }
}

/// No profile: no `_meta`, no extra variables.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProfile;

impl ProfileHook for NoProfile {
    fn session_meta(&self, _agent: &AgentId) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }
}

/// How to start a host. The data folder is `Effective::data_dir()`.
#[non_exhaustive]
pub struct HostConfig {
    effective: Effective,
    launcher: Arc<dyn Launcher>,
    profile: Box<dyn ProfileHook>,
    refused_folders: Vec<PathBuf>,
    home_folder: Option<PathBuf>,
    clock: Arc<dyn Clock>,
    ids: Box<dyn IdSource>,
    patch_window_bytes: usize,
    managed_mcp: PathBuf,
    app_capacity: usize,
    codex_hooks: Option<serde_json::Map<String, serde_json::Value>>,
    pre_write_guard: Option<PathBuf>,
    limits_reader: Option<Arc<dyn ReadLimits>>,
    wiring: LaunchWiring,
}

/// A `HostConfig` taken apart by the host thread.
pub(crate) struct ConfigParts {
    pub(crate) effective: Effective,
    pub(crate) launcher: Arc<dyn Launcher>,
    pub(crate) profile: Box<dyn ProfileHook>,
    pub(crate) refused_folders: Vec<PathBuf>,
    pub(crate) home_folder: Option<PathBuf>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Box<dyn IdSource>,
    pub(crate) patch_window_bytes: usize,
    pub(crate) managed_mcp: PathBuf,
    pub(crate) app_capacity: usize,
    pub(crate) codex_hooks: Option<serde_json::Map<String, serde_json::Value>>,
    pub(crate) pre_write_guard: Option<PathBuf>,
    /// The limits reader a caller set; `None` builds the default at start-up.
    pub(crate) limits_reader: Option<Arc<dyn ReadLimits>>,
    pub(crate) wiring: LaunchWiring,
}

impl HostConfig {
    /// A config with the settings, the launcher that starts agents, and every default: no
    /// profile, no refused folders, the OS's home folder, the system clock and random ids.
    pub fn new(effective: Effective, launcher: Arc<dyn Launcher>) -> Self {
        Self {
            effective,
            launcher,
            profile: Box::new(NoProfile),
            refused_folders: Vec::new(),
            home_folder: SystemDirs.home(),
            clock: Arc::new(SystemClock),
            ids: Box::new(RandomV7::new()),
            patch_window_bytes: PATCH_WINDOW_BYTES,
            app_capacity: SUBSCRIPTION_CAPACITY,
            managed_mcp: crate::agent::loads::managed_mcp_path(),
            codex_hooks: None,
            pre_write_guard: None,
            limits_reader: None,
            wiring: LaunchWiring::default(),
        }
    }

    /// The profile hook (default: [`NoProfile`]).
    pub fn with_profile_hook(mut self, hook: Box<dyn ProfileHook>) -> Self {
        self.profile = hook;
        self
    }

    /// Folders a lawyer may not pick as a matter, from `clauseo_settings::refused_folders`
    /// (default: none). The host adds its data folder.
    pub fn with_refused_folders(mut self, folders: Vec<PathBuf>) -> Self {
        self.refused_folders = folders;
        self
    }

    /// The lawyer's home folder (default: the OS's, from `clauseo_settings::SystemDirs`), for
    /// two things: a matter's `folder_display`, its path below this folder, and telling the
    /// home folder apart from the other refused folders (`matter.folder_is_home`). `None` shows
    /// every path in full and refuses the home folder as a system folder. Tests pass a folder of
    /// their own.
    pub fn with_home_folder(mut self, home: Option<PathBuf>) -> Self {
        self.home_folder = home;
        self
    }

    /// Where to look for Claude Code's enterprise `managed-mcp.json` (default: this OS's path,
    /// `/etc/claude-code/managed-mcp.json` on Linux, `/Library/Application
    /// Support/ClaudeCode/managed-mcp.json` on macOS, `C:\Program
    /// Files\ClaudeCode\managed-mcp.json` on Windows). When a file is there, sessions get no
    /// MCP servers and no strict MCP config: the firm controls MCP. Tests point it at a folder
    /// of their own.
    pub fn with_managed_mcp_path(mut self, path: PathBuf) -> Self {
        self.managed_mcp = path;
        self
    }

    /// The program Claude Code's `PreToolUse` hook runs, with the one argument
    /// [`pre_write_guard::FLAG`](crate::pre_write_guard::FLAG), before each file edit and shell
    /// command (default: none, and no hook goes). The desktop passes its own executable, whose
    /// `main` runs [`pre_write_guard::run`](crate::pre_write_guard::run) on that argument.
    /// `agent/loads.rs` builds the hook entry.
    pub fn with_pre_write_guard(mut self, program: PathBuf) -> Self {
        self.pre_write_guard = Some(program);
        self
    }

    /// The egress proxy that gives each session's browser its port (default: none, so no session
    /// gets the `browser` server and the header shows it unavailable with `NoProxy`).
    pub fn with_egress_proxy(mut self, proxy: Arc<dyn EgressProxy>) -> Self {
        self.wiring.proxy = Some(proxy);
        self
    }

    /// A folder to put first on every agent's `PATH` (default: none): stetdoc's own folder,
    /// `clauseo_profile::ClauseoProfile::path_prepend`, which holds no other program. It goes
    /// before the `PATH` the agent would otherwise get (the hook's, else a settings env `PATH`,
    /// else the inherited one). It shadows nothing the host relies on: the pre-write guard's
    /// hooks run the desktop executable by its absolute path, never through `PATH`.
    pub fn with_agent_path_prepend(mut self, folder: PathBuf) -> Self {
        self.wiring.path_prepend = Some(folder);
        self
    }

    /// The Chrome the browser entry runs (`{browser_executable}`), a stand-in for S27's choice
    /// (default: none, `BrowserNotInstalled`).
    #[cfg(feature = "test-support")]
    pub fn with_browser_executable(mut self, chrome: Option<PathBuf>) -> Self {
        self.wiring.browser_executable = chrome;
        self
    }

    /// Browsers allowed at once and browsers open now, a stand-in for S27's slot count (default:
    /// no limit). With `open >= limit` the browser is `TooManyBrowsers`.
    #[cfg(feature = "test-support")]
    pub fn with_browser_slots(mut self, limit: usize, open: usize) -> Self {
        self.wiring.browser_slots = (limit, open);
        self
    }

    /// The clock (default: [`SystemClock`]).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The id source (default: [`RandomV7`]).
    pub fn with_ids(mut self, ids: Box<dyn IdSource>) -> Self {
        self.ids = ids;
        self
    }

    /// What reads each agent account's plan usage (default: `clauseo_limits::ProcessReaders`
    /// over the settings, the launcher, the host clock and the Codex home the host launches
    /// `codex-acp` with, `<data folder>/agents/codex-acp/home`). The host spawns each read on
    /// its pool and never waits on one.
    pub fn with_limits_reader(mut self, reader: Arc<dyn ReadLimits>) -> Self {
        self.limits_reader = Some(reader);
        self
    }

    /// The byte cap of each stream's patch window, for tests that push a subscriber out of it.
    #[cfg(feature = "test-support")]
    pub fn with_patch_window_bytes(mut self, bytes: usize) -> Self {
        self.patch_window_bytes = bytes;
        self
    }

    /// The capacity of each app subscription's channel (default 1024), for tests that fill one
    /// with a few changes instead of a thousand.
    #[cfg(feature = "test-support")]
    pub fn with_app_subscription_capacity(mut self, capacity: usize) -> Self {
        self.app_capacity = capacity.max(1);
        self
    }

    /// The `hooks` object a Codex agent's `CODEX_CONFIG` carries, as the codex hooks seam
    /// (`actor/turn/agents.rs`) would give it, for tests of the guard's trust check.
    #[cfg(feature = "test-support")]
    pub fn with_codex_hooks(mut self, hooks: serde_json::Map<String, serde_json::Value>) -> Self {
        self.codex_hooks = Some(hooks);
        self
    }

    /// Takes the config apart.
    pub(crate) fn into_parts(self) -> ConfigParts {
        ConfigParts {
            effective: self.effective,
            launcher: self.launcher,
            profile: self.profile,
            refused_folders: self.refused_folders,
            home_folder: self.home_folder,
            clock: self.clock,
            ids: self.ids,
            patch_window_bytes: self.patch_window_bytes,
            app_capacity: self.app_capacity,
            managed_mcp: self.managed_mcp,
            codex_hooks: self.codex_hooks,
            pre_write_guard: self.pre_write_guard,
            limits_reader: self.limits_reader,
            wiring: self.wiring,
        }
    }
}

#[cfg(test)]
mod tests;

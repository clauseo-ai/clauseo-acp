//! What `Host::start` needs: the settings, the launcher, and the parts tests swap out.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_acp::McpServer;
use clauseo_counts::Counts;
use clauseo_limits::ReadLimits;
use clauseo_process::Launcher;
use clauseo_profile::{SkillWrite, SkillsWritten};
use clauseo_protocol::{AgentId, MatterId, ThreadId};
use clauseo_settings::{Effective, PlatformDirs as _, SystemDirs};
use serde_json::{Map, Value};

use crate::browser::chrome::ChromeRoots;
use crate::browser::limits::BrowserSlots;
use crate::browser::{BrowserLaunch, UnsupportedLaunch};
use crate::clock::{Clock, SystemClock};
use crate::ids::{IdSource, RandomV7};
use crate::search::SearchSeams;

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
/// - `launch_writes` are written on the launch-prep thread before the agent starts; the first
///   write's [`SkillsWritten`] goes to that launch's `codex_config` and to no other launch.
///
/// Every method runs on the host thread, at each launch and session start, and must do no I/O:
/// no file, network or process call, and nothing that blocks. It hands back data the adapter
/// already holds; the host does any file work (writing `launch_writes`) off that thread.
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
    /// `written` is this launch's own token, from writing the first of [`Self::launch_writes`]
    /// on its launch-prep thread; `None` when that write failed or there was none. The desktop
    /// adapter hands it to `ClauseoProfile::codex_keys`, and without it calls
    /// `ClauseoProfile::codex_keys_without_index`, so the skills index names only files this
    /// launch wrote (decision record 0031).
    fn codex_config(
        &self,
        _agent: &AgentId,
        _written: Option<&SkillsWritten>,
    ) -> Map<String, Value> {
        Map::new()
    }
    /// The skills writes to run, in order, before `agent` starts (default: none), as
    /// `ClauseoProfile::launch_writes` gives them: Claude's plugin first, for a Codex agent too,
    /// since Codex reads its skills through the index in its instructions. A write failure is a
    /// warning, never a failed launch.
    fn launch_writes(&self, _agent: &AgentId) -> Vec<SkillWrite> {
        Vec::new()
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
/// folder, where to look for Chrome, and the browser slots.
#[derive(Clone)]
pub(crate) struct LaunchWiring {
    pub(crate) proxy: Option<Arc<dyn EgressProxy>>,
    pub(crate) path_prepend: Option<PathBuf>,
    /// A Chrome the tests force, in place of the search.
    pub(crate) browser_executable: Option<PathBuf>,
    /// Where the launch prep looks for Chrome; `None` is this machine's roots
    /// (`ChromeRoots::system` with the data folder as app data).
    pub(crate) chrome_roots: Option<ChromeRoots>,
    /// The browser slots: the limit from the machine's memory, and the sessions holding one.
    pub(crate) browser_slots: BrowserSlots,
    /// macOS and Linux: where the browser socket folders go and the start-up sweep looks;
    /// `None` is `/tmp`. Tests pass a folder of their own.
    #[cfg(unix)]
    pub(crate) browser_socket_root: Option<PathBuf>,
    /// The environment an agent's spec starts from, in place of the host's own
    /// (`std::env::vars_os` at launch), for tests; `None` reads the host's.
    pub(crate) inherited_env: Option<Vec<(OsString, OsString)>>,
}

impl Default for LaunchWiring {
    fn default() -> Self {
        Self {
            proxy: None,
            path_prepend: None,
            browser_executable: None,
            chrome_roots: None,
            browser_slots: BrowserSlots::new(None),
            #[cfg(unix)]
            browser_socket_root: None,
            inherited_env: None,
        }
    }
}

impl std::fmt::Debug for LaunchWiring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchWiring")
            .field("proxy", &self.proxy.is_some())
            .field("path_prepend", &self.path_prepend)
            .field("browser_executable", &self.browser_executable)
            .field("chrome_roots", &self.chrome_roots)
            .field("browser_slots", &self.browser_slots)
            // Names and values stay out: the environment holds secrets.
            .field("inherited_env", &self.inherited_env.is_some())
            .finish_non_exhaustive()
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
    scan_walk: crate::actor::scan::Walk,
    counts: Option<Counts>,
    search: SearchSeams,
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
    pub(crate) scan_walk: crate::actor::scan::Walk,
    /// The usage counts a caller set; `None` counts nothing.
    pub(crate) counts: Option<Counts>,
    /// The search thread's test seams.
    pub(crate) search: SearchSeams,
}

impl HostConfig {
    /// A config with the settings, the launcher that starts agents, and every default: no
    /// profile, no refused folders, the OS's home folder, the system clock and random ids.
    ///
    /// The launcher must clear the environment (`SpawnOptions::clear_env`, or a launcher that
    /// always does, such as the sandbox's): an agent's spec is its whole environment, the host's
    /// own minus the variables no agent may get (`GIT_*`, `SSH_*`, `NO_BROWSER`,
    /// `CLAUDE_CODE_REMOTE`, and Codex's dropped names), and a launcher that lets the child
    /// inherit the host's environment gives every dropped variable back. The plan readers'
    /// specs (`clauseo_limits`) also carry the inherited environment, minus the same drops.
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
            scan_walk: crate::actor::scan::os_walk(),
            counts: None,
            search: SearchSeams::default(),
        }
    }

    /// The anonymous usage counts (default: none, and nothing is counted). The desktop opens
    /// them (`Counts::open`, with `privacy.counts` after policy and whether the first act has
    /// happened) and passes a clone here; the host bumps its call sites' events into it and turns
    /// it on, off or started for `SetPrivacySettings` and `FirstAct` (decision record 0036).
    /// Without it those commands still change the settings, and `counts_started` reads false.
    pub fn with_counts(mut self, counts: Counts) -> Self {
        self.counts = Some(counts);
        self
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

    /// The environment every agent's spec starts from, in place of the host's own, for tests
    /// that check what reaches the child without changing this process's environment.
    #[cfg(feature = "test-support")]
    pub fn with_inherited_env(mut self, env: Vec<(OsString, OsString)>) -> Self {
        self.wiring.inherited_env = Some(env);
        self
    }

    /// A Chrome every session's browser entry runs (`{browser_executable}`), in place of the
    /// launch prep's search (`browser::chrome::find`), for tests. `None` keeps the search.
    #[cfg(feature = "test-support")]
    pub fn with_browser_executable(mut self, chrome: Option<PathBuf>) -> Self {
        self.wiring.browser_executable = chrome;
        self
    }

    /// macOS and Linux: the folder the browser socket folders go in, and the start-up sweep of
    /// stale ones looks in, in place of `/tmp`, for tests.
    #[cfg(all(unix, feature = "test-support"))]
    pub fn with_browser_socket_root(mut self, root: PathBuf) -> Self {
        self.wiring.browser_socket_root = Some(root);
        self
    }

    /// Where the launch prep looks for Chrome (default: this machine's places, with the data
    /// folder as the app data folder that holds Chrome for Testing). Tests pass fixture folders.
    pub fn with_chrome_roots(mut self, roots: ChromeRoots) -> Self {
        self.wiring.chrome_roots = Some(roots);
        self
    }

    /// The machine's total memory in bytes, which sets how many sessions get the browser at once
    /// (`browser::limits::slot_limit`: 2 at 8 GB or less, 4 above). Default: unknown, so 2.
    /// The desktop fills it.
    pub fn with_machine_memory(mut self, bytes: u64) -> Self {
        self.wiring.browser_slots = BrowserSlots::new(Some(bytes));
        self
    }

    /// Where Chrome runs (default: [`BrowserLaunch::InJail`]). Only `InJail` is built:
    /// `OutsideJail`, stage C's switch, is refused for now.
    pub fn with_browser_launch(self, launch: BrowserLaunch) -> Result<Self, UnsupportedLaunch> {
        // `InJail` is what the host does anyway, so it is not stored.
        match launch {
            BrowserLaunch::InJail => Ok(self),
            BrowserLaunch::OutsideJail { cdp_port } => Err(UnsupportedLaunch { cdp_port }),
        }
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

    /// The profile hook set, for tests of a caller's wiring (`crates/desktop`'s `with_profile`).
    #[cfg(feature = "test-support")]
    pub fn profile_hook(&self) -> &dyn ProfileHook {
        self.profile.as_ref()
    }

    /// The folder set by [`Self::with_agent_path_prepend`], for tests of a caller's wiring.
    #[cfg(feature = "test-support")]
    pub fn agent_path_prepend(&self) -> Option<&std::path::Path> {
        self.wiring.path_prepend.as_deref()
    }

    /// The walk folder scans run instead of `clauseo_matters::scan_folder`, for tests that
    /// step a scan by hand.
    #[cfg(test)]
    pub(crate) fn with_scan_walk(mut self, walk: crate::actor::scan::Walk) -> Self {
        self.scan_walk = walk;
        self
    }

    /// The capacity of the commit channel to the search thread (default 1,024), for tests
    /// that make it lag and catch up from the store.
    #[cfg(feature = "test-support")]
    pub fn with_search_channel_capacity(mut self, capacity: usize) -> Self {
        self.search.capacity = capacity.max(1);
        self
    }

    /// Holds that stop the search thread: `rebuild` before each conversation of a rebuild,
    /// `query` before each search runs, each until released.
    #[cfg(feature = "test-support")]
    pub fn with_search_holds(
        mut self,
        rebuild: Option<crate::SearchHold>,
        query: Option<crate::SearchHold>,
    ) -> Self {
        self.search.rebuild_hold = rebuild;
        self.search.query_hold = query;
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
            scan_walk: self.scan_walk,
            counts: self.counts,
            search: self.search,
        }
    }
}

#[cfg(test)]
mod tests;

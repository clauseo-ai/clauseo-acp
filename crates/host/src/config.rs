//! What `Host::start` needs: the settings, the launcher, and the parts tests swap out.

#[cfg(feature = "test-support")]
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_counts::Counts;
use clauseo_limits::ReadLimits;
use clauseo_process::Launcher;
use clauseo_protocol::{AgentId, ErrorCode};
use clauseo_settings::{Effective, PlatformDirs as _, SystemDirs};

use crate::agent::login_link::{LoginCheck, SignInAgain};
use crate::agent::login_splits::LoginSplits;
use crate::browser::chrome::ChromeRoots;
#[cfg(feature = "test-support")]
use crate::browser::install::InstallTools;
use crate::browser::limits::BrowserSlots;
use crate::browser::{BrowserLaunch, UnsupportedLaunch};
use crate::clock::{Clock, SystemClock};
use crate::ids::{IdSource, RandomV7};
use crate::search::SearchSeams;
use crate::update::Updater;

use crate::handle::SUBSCRIPTION_CAPACITY;
use crate::window::PATCH_WINDOW_BYTES;

mod idle;
pub(crate) use idle::IdleStop;
pub use idle::{CAP_MIN_IDLE, IDLE_LIMIT, LIVE_AGENTS};
mod profile_hook;
pub use profile_hook::{NoProfile, ProfileHook};
mod parts;
pub(crate) use parts::{ConfigParts, UpdateSeams};
mod sandbox;
pub use sandbox::{Detect, Sandbox, SandboxLaunch, SandboxLaunchers, SessionEnd};
mod egress;
#[cfg(feature = "test-support")]
pub use egress::TestProxy;
pub use egress::{DEFAULT_TREE_WAIT, EgressProxy};
mod session;
pub use session::SessionKey;
mod wiring;
pub(crate) use wiring::{LaunchWiring, PackReadHook};

pub use crate::actor::turn::PackRequest;

/// How to start a host. The data folder is `Effective::data_dir()`.
#[non_exhaustive]
pub struct HostConfig {
    pub(crate) effective: Effective,
    launcher: Arc<dyn Launcher>,
    profile: Box<dyn ProfileHook>,
    refused_folders: Vec<PathBuf>,
    refused_trees: Vec<PathBuf>,
    documents: Option<crate::actor::documents::Later>,
    home_folder: Option<PathBuf>,
    pub(crate) clock: Arc<dyn Clock>,
    ids: Box<dyn IdSource>,
    patch_window_bytes: usize,
    managed_mcp: PathBuf,
    app_capacity: usize,
    codex_hooks: Option<serde_json::Map<String, serde_json::Value>>,
    folder_check: crate::agent::folder_support::CheckSource,
    pre_write_guard: Option<PathBuf>,
    limits_reader: Option<Arc<dyn ReadLimits>>,
    pub(crate) wiring: LaunchWiring,
    scan_walk: crate::actor::scan::Walk,
    scratch: crate::actor::scratch::Seams,
    folder_probes: crate::actor::FolderProbes,
    counts: Option<Counts>,
    search: SearchSeams,
    update: UpdateSeams,
    idle: IdleStop,
    sandbox: Option<Sandbox>,
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
            refused_trees: Vec::new(),
            documents: None,
            home_folder: SystemDirs.home(),
            clock: Arc::new(SystemClock),
            ids: Box::new(RandomV7::new()),
            patch_window_bytes: PATCH_WINDOW_BYTES,
            app_capacity: SUBSCRIPTION_CAPACITY,
            managed_mcp: crate::agent::loads::managed_mcp_path(),
            codex_hooks: None,
            folder_check: crate::agent::folder_support::CheckSource::default(),
            pre_write_guard: None,
            limits_reader: None,
            wiring: LaunchWiring::default(),
            scan_walk: crate::actor::scan::os_walk(),
            scratch: crate::actor::scratch::Seams::default(),
            folder_probes: crate::actor::FolderProbes::default(),
            counts: None,
            search: SearchSeams::default(),
            update: UpdateSeams::default(),
            idle: IdleStop::default(),
            sandbox: None,
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

    /// The updater (default: [`crate::update::NoUpdater`], a package manages it). The host
    /// checks and downloads through it on its own pool (decision record 0037).
    pub fn with_updater(mut self, updater: Arc<dyn Updater>) -> Self {
        self.update.updater = updater;
        self
    }

    /// The seed of the updater's check delays (default: one taken from the clock at start), so a
    /// test with `ManualClock` knows when each check comes.
    pub fn with_update_jitter_seed(mut self, seed: u64) -> Self {
        self.update.jitter_seed = Some(seed);
        self
    }

    /// The app's version, `0.4.1` (default: this crate's own version, which is not the app's).
    /// Desktop passes its `tauri.conf.json` version: `GetUpdate` shows it, and the marker of an
    /// install compares it at the next start.
    pub fn with_app_version(mut self, version: String) -> Self {
        self.update.app_version = version;
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

    /// Folders a lawyer may not pick as a matter, nor any folder inside them, from
    /// `clauseo_settings::refused_trees` (default: none). The host adds its data folder.
    pub fn with_refused_trees(mut self, trees: Vec<PathBuf>) -> Self {
        self.refused_trees = trees;
        self
    }

    /// The default matters folder comes later (default: it is in the settings from the start):
    /// the host starts without waiting on Documents and takes `later`'s folders when they come
    /// (`Locations::with_documents`). Until then `GetDefaultLocation`, a create or check in the
    /// default location and the Scratch matter wait for them, at most 5 s on the host clock,
    /// then are refused as retryable; a dropped sender leaves no default matters folder.
    pub fn with_documents_later(
        mut self,
        later: futures::channel::oneshot::Receiver<clauseo_settings::Locations>,
    ) -> Self {
        self.documents = Some(later);
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

    /// The egress proxy giving each session's browser its port (default: none: no `browser`
    /// server, shown as `NoProxy`); one opens per launch with the browser ([`EgressProxy`]).
    pub fn with_egress_proxy(mut self, proxy: Arc<dyn EgressProxy>) -> Self {
        self.wiring.proxy = Some(proxy);
        self
    }

    /// The removal wait's cap (default [`DEFAULT_TREE_WAIT`]; `launch_checks::straggler`).
    #[cfg(feature = "test-support")]
    pub fn with_tree_wait(mut self, wait: std::time::Duration) -> Self {
        self.wiring.tree_wait = wait;
        self
    }

    /// Runs as a removal's wait starts, with the session's Chrome port: a test binds it there.
    #[cfg(feature = "test-support")]
    pub fn with_tree_wait_hook(mut self, hook: Arc<dyn Fn(Option<u16>) + Send + Sync>) -> Self {
        self.wiring.tree_wait_hook = Some(hook);
        self
    }

    /// Makes every removal's thread fail to start: a test checks the run's parts are held.
    #[cfg(feature = "test-support")]
    pub fn with_removal_thread_failing(mut self) -> Self {
        self.wiring.removal_thread_fails = true;
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

    /// The launcher agents and plan readers start with, for a caller's tests that launch
    /// through it (desktop checks that its app config's launcher clears the environment).
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn launcher(&self) -> Arc<dyn Launcher> {
        Arc::clone(&self.launcher)
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

    /// The tools the Chrome for Testing download uses (default: `SystemTools`: curl, ditto or
    /// tar, and the OS's signature check), for tests that run it without the network.
    #[cfg(feature = "test-support")]
    pub fn with_browser_install_tools(mut self, tools: Arc<dyn InstallTools>) -> Self {
        self.wiring.install_tools = Some(tools);
        self
    }

    /// What the firm's Chrome, Edge and Chromium `RemoteDebuggingAllowed` says, for tests that
    /// pass a fake (default under `test-support`: nothing set; in production, this machine's
    /// registry, managed preferences or policy folders).
    #[cfg(feature = "test-support")]
    pub fn with_remote_debugging_policy(
        mut self,
        policy: Arc<dyn clauseo_settings::chromium_policy::RemoteDebuggingPolicy>,
    ) -> Self {
        self.wiring.rd_policy = Arc::new(crate::browser::rd_policy::RdPolicy::new(policy));
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
        self.idle.memory = Some(bytes);
        self
    }

    /// How long an agent with nothing pending stays live (default [`IDLE_LIMIT`], 300 s).
    pub fn with_idle_limit(mut self, limit: std::time::Duration) -> Self {
        self.idle.limit = limit;
        self
    }

    /// Agents live at once on a machine of 8 GB or less (default [`LIVE_AGENTS`], 2).
    pub fn with_live_agents(mut self, agents: usize) -> Self {
        self.idle.live_agents = agents;
        self
    }

    /// How long an agent must be idle before the live-agents cap may stop it for another
    /// thread's launch (default [`CAP_MIN_IDLE`], 60 s); with none, the launch passes the cap.
    pub fn with_cap_min_idle(mut self, min_idle: std::time::Duration) -> Self {
        self.idle.cap_min_idle = min_idle;
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

    /// The store's close reports a failure once it has closed, for the updater's tests of an
    /// install whose shutdown did not end cleanly.
    #[cfg(feature = "test-support")]
    pub fn with_failing_close(mut self) -> Self {
        self.update.fail_close = true;
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

    /// A fixed answer for every launch's folder check (`agent/folder_support.rs`) in place of
    /// the Windows check, so tests of what follows from it (Codex refused, Claude's forced ask)
    /// run on every OS. `None` is a fine folder. `HostQuery::FolderSupport` answers from it
    /// too; matter creation still runs the real check.
    #[cfg(feature = "test-support")]
    pub fn with_folder_support(
        mut self,
        kind: Option<clauseo_protocol::FolderSupportKind>,
    ) -> Self {
        self.folder_check = crate::agent::folder_support::CheckSource::Fixed(
            crate::agent::folder_support::Answer::Known(kind),
        );
        self
    }

    /// A failed folder check for every launch (an io error on Windows), as
    /// [`Self::with_folder_support`] fixes an answer: Claude fails closed into forced ask, Codex
    /// is not refused, the row shows `check_failed`.
    #[cfg(feature = "test-support")]
    pub fn with_folder_check_failed(mut self) -> Self {
        self.folder_check = crate::agent::folder_support::CheckSource::Fixed(
            crate::agent::folder_support::Answer::Failed,
        );
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

    /// The scan the Scratch matter's folder is found by at start-up instead of
    /// `scratch::free_folder`, for tests that hold it.
    #[cfg(test)]
    pub(crate) fn with_scratch_scan(mut self, scan: crate::actor::scratch::Scan) -> Self {
        self.scratch.scan = scan;
        self
    }

    /// Shutdown waits for the Scratch scan too, and says so on `told` each time it does, for
    /// tests that answer the scan while the host stops.
    #[cfg(test)]
    pub(crate) fn with_scratch_stop_held(mut self, told: std::sync::mpsc::Sender<()>) -> Self {
        self.scratch.stop_held = Some(told);
        self
    }

    /// The folder probes run off the actor (a new matter's folder, a located file's) instead of
    /// the file system's (`actor/matters/probe.rs`), for tests that hold them.
    #[cfg(test)]
    pub(crate) fn with_folder_probes(mut self, probes: crate::actor::FolderProbes) -> Self {
        self.folder_probes = probes;
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

    /// The sign-in hook (decision record 0048, ruling 2): asked off the actor at every Codex
    /// launch, with a bounded wait, whether Clauseo's Codex home shares the lawyer's login file
    /// (`agent/login_link.rs`). Unset (the default), nothing is checked: no split check and no
    /// guard.
    pub fn with_login_check(mut self, check: Arc<dyn LoginCheck + Send + Sync>) -> Self {
        self.wiring.sign_in.check = Some(check);
        self
    }

    /// The case-law connector's relays (R24): each agent launch whose profile offers a
    /// `case-law` entry, with `tools.connectors` on, no firm `managed-mcp.json` and the
    /// connector signed in, gets a relay of its own and the entry. Unset (the default), no session
    /// gets case law. Desktop builds the relays from the `Connectors` it passes to
    /// [`Self::with_connectors`], with that wiring's `ConnectorEvents` as their events.
    pub fn with_case_law_relays(mut self, relays: crate::connectors::Relays) -> Self {
        self.wiring.relays = Some(relays);
        self
    }

    /// The connector commands' wiring (R24): the app's `Connectors`, the events sink its relays
    /// were built with, and the opener for the sign-in page. Unset (the default), every connector
    /// command is refused and no app subscriber hears `ConnectorsChanged`.
    pub fn with_connectors(mut self, wiring: crate::connectors::ConnectorsWiring) -> Self {
        self.wiring.connectors = Some(wiring);
        self
    }

    /// The sign-in-again test (S18's `is_sign_in_again`): a turn whose agent error's text it
    /// accepts fails as `agent.auth_required`, so its turn is held for the sign-in. Unset (the
    /// default), only `agent.auth_required` itself is.
    pub fn with_sign_in_again(mut self, again: SignInAgain) -> Self {
        self.wiring.sign_in.again = Some(again);
        self
    }

    /// Refuses `agent`'s new launches with `code` from the host's start, before any launch
    /// can come (decision record 0048): `signin.policy_unreadable` for an agent an unreadable
    /// policy blocks, `agent.keys_loading` until desktop has loaded the stored API keys (and
    /// counted as `agent.key_unreadable` past its bound). As
    /// `HostHandle::set_launch_refused` takes it, which changes or lifts it later. Another code
    /// is a bug: debug builds assert, and release builds log it and set nothing.
    #[must_use]
    pub fn with_launch_refused(mut self, agent: AgentId, code: ErrorCode) -> Self {
        debug_assert!(
            crate::agent::launch_refusals::taken(code),
            "with_launch_refused takes no {code:?}"
        );
        if let Err(error) = self.wiring.sign_in.refused.set(agent, Some(code)) {
            tracing::warn!(code = error.code().as_str(), "a launch refusal was not set");
        }
        self
    }

    /// Desktop's key retry (decision record 0048): called, on the host's actor, with the agent
    /// of each launch that meets `agent.key_unreadable`, before the launch waits for the
    /// refusal to lift; desktop reads that key again and lifts the refusal when it reads. It
    /// must not block.
    #[must_use]
    pub fn with_key_retry(mut self, retry: Arc<dyn Fn(&AgentId) + Send + Sync>) -> Self {
        self.wiring.sign_in.refused.set_key_retry(retry);
        self
    }

    /// A hook run, with the pack's id, between the two reads of a skill pack that
    /// `SetSkillPack` and `ListSkillPacks` make off the host thread: a test changes the pack
    /// there, or asks which thread reads.
    #[cfg(feature = "test-support")]
    pub fn with_pack_read_hook(mut self, hook: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        self.wiring.pack_read_hook = Some(hook);
        self
    }

    /// A hook run off the host thread with a session's run folder once its agent has ended
    /// (its case-law token revoked) and just before the folder is removed: a test checks what
    /// the session left at that moment.
    #[cfg(feature = "test-support")]
    pub fn with_run_removal_hook(
        mut self,
        hook: Arc<dyn Fn(&std::path::Path) + Send + Sync>,
    ) -> Self {
        self.wiring.run_removal_hook = Some(hook);
        self
    }

    /// The sandbox seam (default: none: the computer is `Unsupported { why: not_built }`, and
    /// `RetryProtection` and `RemoveSandboxSetup` are `command.invalid`). Without launchers
    /// (`Sandbox::with_launchers`) it never gates launches (R17 PR-D3a): the computer stays
    /// `not_built`, the sandbox's state goes in the view's `set_up_state`. First detection: in
    /// `Host::start`, on the calling thread; later ones off the host's thread.
    #[must_use]
    pub fn with_sandbox(mut self, sandbox: Sandbox) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// The seam set, for the first detection in `Host::start`.
    pub(crate) fn sandbox(&self) -> Option<&Sandbox> {
        self.sandbox.as_ref()
    }

    /// The receiving end of the login splits, for `HostHandle::login_splits`.
    pub(crate) fn login_splits(&self) -> LoginSplits {
        self.wiring.sign_in.receiver()
    }
}

#[cfg(test)]
mod tests;

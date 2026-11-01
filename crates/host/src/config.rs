//! What `Host::start` needs: the settings, the launcher, and the parts tests swap out.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_process::Launcher;
use clauseo_protocol::AgentId;
use clauseo_settings::Effective;
use serde_json::Value;

use crate::clock::{Clock, SystemClock};
use crate::ids::{IdSource, RandomV7};

use crate::handle::SUBSCRIPTION_CAPACITY;
use crate::window::PATCH_WINDOW_BYTES;

/// What a firm's profile adds to an agent's session and launch. The host calls it at each
/// session start: `session_meta` becomes the `_meta` of `session/new` and `session/load`, and
/// `launch_env` fills the hook slot of the agent's launch. The host then sets the keys that keep
/// out MCP servers the lawyer did not turn on, over whatever the hook returned
/// (`agent/loads.rs` lists them).
pub trait ProfileHook: Send {
    /// The `_meta` object for `session/new` and `session/load` with `agent`, or `None` for none.
    fn session_meta(&self, agent: &AgentId) -> Option<serde_json::Map<String, Value>>;
    /// Variables set on top of the agent's own when it launches.
    fn launch_env(&self, agent: &AgentId) -> BTreeMap<OsString, OsString>;
}

/// No profile: no `_meta`, no extra variables.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProfile;

impl ProfileHook for NoProfile {
    fn session_meta(&self, _agent: &AgentId) -> Option<serde_json::Map<String, Value>> {
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
    clock: Arc<dyn Clock>,
    ids: Box<dyn IdSource>,
    patch_window_bytes: usize,
    managed_mcp: PathBuf,
    app_capacity: usize,
    codex_hooks: Option<serde_json::Map<String, serde_json::Value>>,
    pre_write_guard: Option<PathBuf>,
}

/// A `HostConfig` taken apart by the host thread.
pub(crate) struct ConfigParts {
    pub(crate) effective: Effective,
    pub(crate) launcher: Arc<dyn Launcher>,
    pub(crate) profile: Box<dyn ProfileHook>,
    pub(crate) refused_folders: Vec<PathBuf>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Box<dyn IdSource>,
    pub(crate) patch_window_bytes: usize,
    pub(crate) managed_mcp: PathBuf,
    pub(crate) app_capacity: usize,
    pub(crate) codex_hooks: Option<serde_json::Map<String, serde_json::Value>>,
    pub(crate) pre_write_guard: Option<PathBuf>,
}

impl HostConfig {
    /// A config with the settings, the launcher that starts agents, and every default: no
    /// profile, no refused folders, the system clock and random ids.
    pub fn new(effective: Effective, launcher: Arc<dyn Launcher>) -> Self {
        Self {
            effective,
            launcher,
            profile: Box::new(NoProfile),
            refused_folders: Vec::new(),
            clock: Arc::new(SystemClock),
            ids: Box::new(RandomV7::new()),
            patch_window_bytes: PATCH_WINDOW_BYTES,
            app_capacity: SUBSCRIPTION_CAPACITY,
            managed_mcp: crate::agent::loads::managed_mcp_path(),
            codex_hooks: None,
            pre_write_guard: None,
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
            clock: self.clock,
            ids: self.ids,
            patch_window_bytes: self.patch_window_bytes,
            app_capacity: self.app_capacity,
            managed_mcp: self.managed_mcp,
            codex_hooks: self.codex_hooks,
            pre_write_guard: self.pre_write_guard,
        }
    }
}

#[cfg(test)]
mod tests;

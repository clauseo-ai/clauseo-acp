//! The launch profile's hook (decision record 0030), [`ProfileHook`], and its stand-in that adds
//! nothing, [`NoProfile`]. Kept apart from `config.rs`'s `HostConfig`, which holds one.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use clauseo_acp::McpServer;
use clauseo_profile::{PackWrite, PacksWritten, SkillWrite, SkillsWritten};
use clauseo_protocol::AgentId;
use serde_json::{Map, Value};

use super::PackRequest;

/// A launch's wait for [`ProfileHook::launch_wait`]'s step: it ends with the skills writes to
/// run.
pub type LaunchWait = Pin<Box<dyn Future<Output = LaunchSkills> + Send>>;

/// A launch's skills writes and the profile generation ([`ProfileHook::generation`]) they are
/// of, read no later than the writes.
#[derive(Debug, Clone, Default)]
pub struct LaunchSkills {
    /// The generation, read before the writes.
    pub generation: u64,
    /// The writes.
    pub writes: Vec<SkillWrite>,
}

/// A wait for the profile to change ([`ProfileHook::changed`]): it ends with the new
/// generation.
pub type ProfileChange = Pin<Box<dyn Future<Output = u64> + Send>>;

/// The longest a launch waits for [`ProfileHook::launch_wait`], from the launch: the turn
/// stays "starting" meanwhile.
pub const LAUNCH_WAIT_LIMIT: Duration = Duration::from_secs(20);

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
///   While [`ProfileHook::launch_wait`] gives a wait, the launch's prep waits for it, at most
///   [`LAUNCH_WAIT_LIMIT`], and writes what it ends with instead.
/// - `agent_path` folders go on the session's `PATH` after stetdoc's and the shell browser's.
/// - `pack_write` turns the launch's [`PackRequest`] (decision record 0040) into the packs the
///   launch-prep thread loads and writes into `run/<session id>/skill-packs/`; the
///   [`PacksWritten`] it gets goes to that launch's `session_meta` and `codex_config` only.
///
/// Every method runs on the host thread, at each launch and session start, and must do no I/O:
/// no file, network or process call, and nothing that blocks. It hands back data the adapter
/// already holds; the host does any file work (writing `launch_writes`) off that thread.
pub trait ProfileHook: Send {
    /// The `_meta` object for `session/new` and `session/load` with `agent`, or `None` for none.
    /// `packs` is this launch's own pack plugin, written on its launch-prep thread; `None` when
    /// it has none. The desktop adapter names it as Claude's second plugin
    /// (`ClauseoProfile::session_meta_with_packs`).
    fn session_meta(
        &self,
        agent: &AgentId,
        packs: Option<&PacksWritten>,
    ) -> Option<Map<String, Value>>;
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
    /// launch wrote (decision record 0031). `packs` is this launch's own pack plugin, so the
    /// pack lines name exactly the files it wrote (`ClauseoProfile::codex_keys_with_packs`).
    fn codex_config(
        &self,
        _agent: &AgentId,
        _written: Option<&SkillsWritten>,
        _packs: Option<&PacksWritten>,
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
    /// A wait for a background step in flight that may change [`Self::launch_writes`] (default:
    /// none): desktop's while stetdoc's skill loads after an install or an update. The future
    /// ends when the step ends, with `agent`'s writes as they are then; the launch's prep task
    /// (never this thread) waits for it at most [`LAUNCH_WAIT_LIMIT`] from the launch, then
    /// writes what it ended with, or on the limit what `launch_writes` gave. `None`: nothing
    /// is in flight, and the launch does not wait. No I/O here: it only hands out the wait.
    fn launch_wait(&self, _agent: &AgentId) -> Option<LaunchWait> {
        None
    }
    /// The profile's generation (default: 0): it grows each time the profile the other methods
    /// read changes, as desktop's does when stetdoc's background try puts a new one in place.
    /// The host reads it before a launch's skills writes, and an agent launched on an older
    /// generation is marked to relaunch once [`Self::changed`] tells of a newer one: its next
    /// question relaunches it on the new profile, and with none it stops at the idle limit,
    /// never mid-turn. A lagging one is safe: when [`Self::changed`] ends while this still
    /// says the old value, the host takes the lower of the two and only watches again, so it
    /// never relaunches in a loop. No I/O.
    fn generation(&self) -> u64 {
        0
    }
    /// A wait that ends once the generation is past `seen` (default: none, it never changes).
    /// Awaited on a host task, never this thread. No I/O here: it only hands out the wait.
    fn changed(&self, _seen: u64) -> Option<ProfileChange> {
        None
    }
    /// The skill packs to load and write before `agent` starts, from `request` (default:
    /// none): `ClauseoProfile::pack_write` adds where the packs live. A load or write failure
    /// is a warning, never a failed launch.
    fn pack_write(&self, _agent: &AgentId, _request: &PackRequest) -> Option<PackWrite> {
        None
    }
    /// Folders for `agent`'s session `PATH`, in order (default: none): after stetdoc's folder
    /// and the shell browser's ([`Self::browser_tool`]), before the user's `PATH`. Desktop gives
    /// Clauseo's Python runtime here, for Claude and Codex once it is installed (decision record
    /// 0048). Only absolute folders go on; a relative one is skipped. When any go on, the
    /// session drops `PYTHONHOME` and `PYTHONPATH`. Sessions only: no sign-in, status or limits
    /// run takes them. No I/O: it hands back folders the adapter already holds, looked up off
    /// this thread.
    fn agent_path(&self, _agent: &AgentId) -> Vec<PathBuf> {
        Vec::new()
    }
    /// The browser as a shell tool (default: none): the folder holding the `agent-browser`
    /// command alone (`clauseo_profile::ProfilePaths::tools`). A session whose launch prep finds
    /// a Chrome (`tools.browser` on, the firm's browser policy allowing it) gets it first on its
    /// agent's `PATH` after stetdoc's (before [`Self::agent_path`]'s), agent-browser's variables
    /// (`agent::loads::browser::shell_env`) and the instructions' browser part; any other session
    /// gets none of them.
    fn browser_tool(&self) -> Option<PathBuf> {
        None
    }
    /// The `clauseo-mcp` binary (default: none), whose `launch` mode starts each of the lawyer's
    /// local MCP servers from a private file in the run folder, so its arguments and variables
    /// stay off the agent's command line (`actor/turn/mcp_launch.rs`). With none, the servers
    /// go as the lawyer saved them, with a warning. No I/O: a path the adapter already holds.
    fn mcp_launcher(&self) -> Option<PathBuf> {
        None
    }
}

/// No profile: no `_meta`, no extra variables.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProfile;

impl ProfileHook for NoProfile {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&PacksWritten>,
    ) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }
}

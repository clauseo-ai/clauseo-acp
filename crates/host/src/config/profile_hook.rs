//! The launch profile's hook (decision record 0030), [`ProfileHook`], and its stand-in that adds
//! nothing, [`NoProfile`]. Kept apart from `config.rs`'s `HostConfig`, which holds one.

use std::collections::BTreeMap;
use std::ffi::OsString;

use clauseo_acp::McpServer;
use clauseo_profile::{PackWrite, PacksWritten, SkillWrite, SkillsWritten};
use clauseo_protocol::AgentId;
use serde_json::{Map, Value};

use super::PackRequest;

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
    /// The skill packs to load and write before `agent` starts, from `request` (default:
    /// none): `ClauseoProfile::pack_write` adds where the packs live. A load or write failure
    /// is a warning, never a failed launch.
    fn pack_write(&self, _agent: &AgentId, _request: &PackRequest) -> Option<PackWrite> {
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

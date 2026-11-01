//! What an agent's session loads: the profile hook's `_meta` and launch variables, with the
//! switches that keep out every MCP server but those the lawyer turned on, and those servers.
//!
//! The switches are Claude Code's (claude-agent-acp 0.89.1, Claude Code 2.1.293). They go to
//! every agent, whatever its id: codex-acp and `fake_agent` ignore `_meta.claudeCode`, and
//! [`CLAUDE_AI_MCP_VAR`] means nothing to them. Each merge runs once per session start.
//!
//! The profile hook is Clauseo's own code, not the user's. Anything else it puts under
//! `claudeCode.options` (servers in `mcpServers`, flags in `extraArgs`, a `settings` object) is
//! kept as it is: a deliberate Clauseo choice. Only the keys [`session_meta`] lists are forced
//! over it or taken out: the hook cannot widen `settingSources` (clauseo-acp-private#185), not
//! even through `extraArgs`.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_acp::McpServer;
use clauseo_settings::Effective;
use serde_json::{Map, Value};

/// Claude Code's `settingSources` for every Clauseo session: the user's own settings, so their
/// login (an `apiKeyHelper`, a provider's variables) keeps working. Claude Code then reads none
/// of the matter folder's project and local settings: not its `.mcp.json`, hooks or permission
/// rules. claude-agent-acp reads those files itself, whatever `settingSources` says, and takes a
/// starting mode from them: the SDK drops `acceptEdits` only from the project file, so a
/// folder's `.claude/settings.local.json` can still start the session in accept-edits
/// (clauseo-acp-private#218, older than this setting). A live run on the Mac mini showed `[]`
/// breaking the login (2026-11-02). The user's plugins still load their skills, hooks and agents
/// under `["user"]`; strict MCP config is what keeps their MCP servers out.
/// `tests/scenarios/mcp-defaults.jsonl` pins this value.
pub(crate) const CLAUDE_SETTING_SOURCES: &[&str] = &["user"];

/// `settingSources` when the firm's `managed-mcp.json` is present ([`McpControl::Managed`]):
/// the same user settings, for the login. Strict MCP config cannot go there. A file that parses
/// takes exclusive control: user-scope and plugin servers never load, even with `["user"]`
/// (CLI 2.1.293, read from its source: research-claude.md, "Follow-up: managed paths"; not run
/// live). claude.ai connectors stay off too, since [`CLAUDE_AI_MCP_VAR`] is still `false`.
pub(crate) const CLAUDE_MANAGED_SETTING_SOURCES: &[&str] = &["user"];

/// The variable that, set to `false`, keeps claude.ai's connectors out of a Claude Code session.
pub(crate) const CLAUDE_AI_MCP_VAR: &str = "ENABLE_CLAUDEAI_MCP_SERVERS";

/// Who decides which MCP servers a session gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpControl {
    /// Clauseo: only the servers the lawyer turned on, under strict MCP config.
    Clauseo,
    /// The firm, through Claude Code's enterprise `managed-mcp.json` on this machine. Claude
    /// Code then refuses to start with strict MCP config, and fails the session start when it
    /// is sent any MCP server, so neither is sent, not even a server the hook put in
    /// `options.mcpServers`: the firm controls MCP here. Refusing the session with a clear
    /// message instead would also go here; it is not built.
    Managed,
}

/// Where Claude Code reads the enterprise `managed-mcp.json` on this OS (CLI 2.1.293; no
/// variable moves it).
pub(crate) fn managed_mcp_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    return PathBuf::from("/Library/Application Support/ClaudeCode/managed-mcp.json");
    #[cfg(windows)]
    return PathBuf::from(r"C:\Program Files\ClaudeCode\managed-mcp.json");
    #[cfg(not(any(windows, target_os = "macos")))]
    return PathBuf::from("/etc/claude-code/managed-mcp.json");
}

/// [`McpControl::Managed`] when a file is at `managed` (one `stat`), else
/// [`McpControl::Clauseo`]. Claude Code counts a file that does not parse as present too.
pub(crate) fn mcp_control(managed: &Path) -> McpControl {
    if std::fs::metadata(managed).is_ok() {
        tracing::warn!(
            path = %managed.display(),
            "an enterprise managed-mcp.json is present: the firm controls MCP on this machine, \
             so the session gets no MCP servers and no strict MCP config"
        );
        McpControl::Managed
    } else {
        McpControl::Clauseo
    }
}

/// The `_meta` for `session/new` and `session/load`: the hook's, with these keys set over it.
/// Every other key the hook set stays, under `claudeCode.options` and its `env` too; a hook
/// value at `claudeCode`, `options` or `env` that is not an object is replaced.
///
/// - `claudeCode.options.env.ENABLE_CLAUDEAI_MCP_SERVERS`: `"false"`.
/// - `claudeCode.options.extraArgs`: `setting-sources` taken out, in both forms the SDK turns
///   into a flag (`{"setting-sources": "…"}` and `{"setting-sources=…": null}`), since an extra
///   flag goes after the SDK's own `--setting-sources` and would win.
/// - Under [`McpControl::Clauseo`]: `strictMcpConfig: true` and `settingSources`
///   [`CLAUDE_SETTING_SOURCES`].
/// - Under [`McpControl::Managed`]: `strictMcpConfig: false`, `settingSources`
///   [`CLAUDE_MANAGED_SETTING_SOURCES`], and no `mcpServers`.
pub(crate) fn session_meta(
    hook: Option<Map<String, Value>>,
    control: McpControl,
) -> Map<String, Value> {
    let mut meta = hook.unwrap_or_default();
    let mut claude = take_object(&mut meta, "claudeCode");
    let mut options = take_object(&mut claude, "options");
    let mut env = take_object(&mut options, "env");
    env.insert(CLAUDE_AI_MCP_VAR.to_owned(), Value::from("false"));
    options.insert("env".to_owned(), Value::Object(env));
    let (strict, sources) = match control {
        McpControl::Clauseo => (true, CLAUDE_SETTING_SOURCES),
        McpControl::Managed => (false, CLAUDE_MANAGED_SETTING_SOURCES),
    };
    if let Some(Value::Object(extra)) = options.get_mut("extraArgs") {
        extra.retain(|flag, _| !is_setting_sources_flag(flag));
    }
    if control == McpControl::Managed {
        options.remove("mcpServers");
    }
    options.insert("strictMcpConfig".to_owned(), Value::Bool(strict));
    options.insert("settingSources".to_owned(), Value::from(sources.to_vec()));
    claude.insert("options".to_owned(), Value::Object(options));
    meta.insert("claudeCode".to_owned(), Value::Object(claude));
    meta
}

/// Whether the `extraArgs` key `flag` becomes `--setting-sources`: the SDK writes `--<key>`, then
/// the value unless it is `null`.
fn is_setting_sources_flag(flag: &str) -> bool {
    flag == SETTING_SOURCES_FLAG
        || flag
            .strip_prefix(SETTING_SOURCES_FLAG)
            .is_some_and(|rest| rest.starts_with('='))
}

/// The Claude Code flag `settingSources` becomes.
const SETTING_SOURCES_FLAG: &str = "setting-sources";

/// The object at `key`, taken out of `map`; empty when it is missing or not an object.
fn take_object(map: &mut Map<String, Value>, key: &str) -> Map<String, Value> {
    match map.remove(key) {
        Some(Value::Object(inner)) => inner,
        _ => Map::new(),
    }
}

/// The servers a session gets: under [`McpControl::Clauseo`] the enabled ones, in name order,
/// as stdio servers; under [`McpControl::Managed`] none.
pub(crate) fn mcp_servers(effective: &Effective, control: McpControl) -> Vec<McpServer> {
    match control {
        McpControl::Managed => Vec::new(),
        McpControl::Clauseo => effective
            .enabled_mcp_servers()
            .map(|(name, server)| McpServer::Stdio {
                name: name.to_owned(),
                command: PathBuf::from(&server.command),
                args: server.args.clone(),
                env: server
                    .env
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
            })
            .collect(),
    }
}

/// The variables for `launch_spec`'s hook slot: the hook's, then [`CLAUDE_AI_MCP_VAR`] as
/// `false`, replacing any hook variable of that name in any case.
pub(crate) fn launch_env(mut hook: BTreeMap<OsString, OsString>) -> BTreeMap<OsString, OsString> {
    hook.retain(|name, _| !name.eq_ignore_ascii_case(CLAUDE_AI_MCP_VAR));
    hook.insert(OsString::from(CLAUDE_AI_MCP_VAR), OsString::from("false"));
    hook
}

//! What an agent's session loads: the profile hook's `_meta` and launch variables, with the
//! switches that keep out every MCP server but those the lawyer turned on, and those servers.
//!
//! The switches are Claude Code's (claude-agent-acp 0.89.1, Claude Code 2.1.293). They go to
//! every agent, whatever its id: codex-acp and `fake_agent` ignore `_meta.claudeCode`, and
//! [`CLAUDE_AI_MCP_VAR`] means nothing to them. Each merge runs once per session start.
//!
//! The profile hook is Clauseo's own code, not the user's. Anything else it puts under
//! `claudeCode.options` (flags in `extraArgs`, a `settings` object) is kept as it is: a
//! deliberate Clauseo choice. Only the keys [`session_meta`] lists are forced over it or taken
//! out: the hook cannot widen `settingSources` (clauseo-acp-private#185), not even through
//! `extraArgs`, and its `mcpServers` is removed in every control mode, since servers come only
//! through `ProfileHook::mcp_servers` ([`browser`]). What the launch profile adds per agent kind
//! is in [`profile`].

pub(crate) mod browser;
mod codex;
pub(crate) mod profile;
#[cfg(test)]
mod profile_tests;
#[cfg(test)]
mod tests;

pub use codex::codex_hook_config;

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
///   [`CLAUDE_MANAGED_SETTING_SOURCES`], and no `mcp-config` or `strict-mcp-config` in
///   `extraArgs` (either makes Claude Code refuse to start there).
/// - In every mode: no `mcpServers`. A hook cannot add servers through `_meta`.
/// - With `keys` (a Claude agent): [`profile::claude_options`], before the guard's entry, so
///   the guard's `hooks` and `disableAllHooks` go over whatever settings it merged.
/// - With a `guard` ([`GuardHook`]): [`guard_entry`] appended to
///   `claudeCode.options.settings.hooks.PreToolUse`, after any entries the hook put there, and
///   `settings.disableAllHooks: false`, so a user's own `disableAllHooks` (user settings load)
///   cannot turn it off: `--settings` outranks user settings (Claude Code's settings
///   precedence). It also turns the user's own hooks back on when their `disableAllHooks` had
///   them off. A hook value at `settings`, `hooks` or `PreToolUse` of another type is
///   replaced.
pub(crate) fn session_meta(
    hook: Option<Map<String, Value>>,
    control: McpControl,
    guard: Option<GuardHook<'_>>,
    keys: Option<&profile::ClaudeKeys<'_>>,
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
        extra.retain(|flag, _| {
            let taken_out = is_flag(flag, SETTING_SOURCES_FLAG)
                || (control == McpControl::Managed
                    && MANAGED_MCP_FLAGS.iter().any(|name| is_flag(flag, name)));
            !taken_out
        });
    }
    options.remove("mcpServers");
    options.insert("strictMcpConfig".to_owned(), Value::Bool(strict));
    options.insert("settingSources".to_owned(), Value::from(sources.to_vec()));
    if let Some(keys) = keys {
        profile::claude_options(&mut options, keys);
    }
    if let Some(entry) = guard.and_then(|guard| guard_entry(guard.program, guard.matter)) {
        let mut settings = take_object(&mut options, "settings");
        let mut hooks = take_object(&mut settings, "hooks");
        let mut pre_tool_use = match hooks.remove("PreToolUse") {
            Some(Value::Array(entries)) => entries,
            _ => Vec::new(),
        };
        pre_tool_use.push(entry);
        hooks.insert("PreToolUse".to_owned(), Value::Array(pre_tool_use));
        settings.insert("hooks".to_owned(), Value::Object(hooks));
        settings.insert("disableAllHooks".to_owned(), Value::Bool(false));
        options.insert("settings".to_owned(), Value::Object(settings));
    }
    claude.insert("options".to_owned(), Value::Object(options));
    meta.insert("claudeCode".to_owned(), Value::Object(claude));
    meta
}

/// Whether the `extraArgs` key `flag` becomes `--<name>`: the SDK writes `--<key>`, then the
/// value unless it is `null`, so `name` and `name=…` both do.
fn is_flag(flag: &str, name: &str) -> bool {
    flag.strip_prefix(name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('='))
}

/// The Claude Code flag `settingSources` becomes.
const SETTING_SOURCES_FLAG: &str = "setting-sources";

/// The flags that make Claude Code refuse to start while `managed-mcp.json` is present: a
/// server config, and strict MCP config.
const MANAGED_MCP_FLAGS: [&str; 2] = ["mcp-config", "strict-mcp-config"];

/// What the pre-write guard's hook for a session needs: the program to run and the matter
/// folder it guards.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GuardHook<'a> {
    /// The program (`HostConfig::with_pre_write_guard`).
    pub(crate) program: &'a Path,
    /// The session's matter folder, passed as `--matter`.
    pub(crate) matter: &'a Path,
}

/// The tools the pre-write guard's hook runs before: Claude Code's file tools (`MultiEdit` is
/// only a legacy alias at 2.1.293, harmless to keep), `Bash` and `PowerShell` (on by default on
/// Windows for some accounts), which the guard checks by a heuristic. A matcher of names joined
/// by `|` is an exact list.
pub(crate) const GUARD_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit|Bash|PowerShell";

/// The seconds Claude Code waits for the guard before going on without it (default 600).
pub(crate) const GUARD_TIMEOUT_SECONDS: u64 = 10;

/// The `PreToolUse` entry that runs `program` with [`pre_write_guard::FLAG`]
/// (`crate::pre_write_guard`) and `--matter <matter>` before each of [`GUARD_MATCHER`]'s tools.
/// The matter folder goes as an argument because the hook input's `cwd` follows the agent's
/// `cd`. `None`, with a warning, when `program` or `matter` is no UTF-8 string: JSON cannot
/// carry it, and a lossy copy could run another program or guard another folder.
///
/// It goes in Claude Code's flag settings (`options.settings`), which load whatever the
/// `settingSources`. It uses the exec form (`command` plus `args`): Claude Code spawns
/// `command` directly with `args`, with no shell on any OS, so the path needs no quoting for
/// `/bin/sh`, Git Bash or PowerShell (CLI 2.1.293, the one bundled with claude-agent-acp
/// 0.89.1: the command hook schema's `args`, "spawned directly with these arguments, no shell",
/// and the runner's `spawn(command, args)` without `shell` when `args` is set;
/// research-claude.md's `cli-embedded.js.txt` at characters 972200 and 11214500). Its one
/// risk: a CLI that predates the exec form would drop `args` and run `command` alone through
/// its shell, which starts the app on every edit. The CLI comes bundled with the pinned
/// adapter, so that needs an adapter downgrade.
///
/// [`pre_write_guard::FLAG`]: crate::pre_write_guard::FLAG
pub(crate) fn guard_entry(program: &Path, matter: &Path) -> Option<Value> {
    let (Some(command), Some(matter)) = (program.to_str(), matter.to_str()) else {
        tracing::warn!(
            program = %program.display(),
            matter = %matter.display(),
            "the pre-write guard's program or matter path is not UTF-8: no guard hook goes to \
             the agent"
        );
        return None;
    };
    Some(serde_json::json!({
        "matcher": GUARD_MATCHER,
        "hooks": [{
            "type": "command",
            "command": command,
            "args": [
                crate::pre_write_guard::FLAG,
                crate::pre_write_guard::MATTER_FLAG,
                matter,
            ],
            "timeout": GUARD_TIMEOUT_SECONDS,
        }],
    }))
}

/// The object at `key`, taken out of `map`; empty when it is missing or not an object.
pub(crate) fn take_object(map: &mut Map<String, Value>, key: &str) -> Map<String, Value> {
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

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
pub(crate) mod case_law;
#[cfg(test)]
mod claude_browser_tests;
mod codex;
#[cfg(test)]
mod codex_entry_tests;
pub(crate) mod lawyer;
#[cfg(test)]
mod lawyer_tests;
#[cfg(test)]
mod layering_tests;
pub(crate) mod profile;
#[cfg(test)]
mod profile_keys_tests;
#[cfg(test)]
mod profile_tests;
pub(crate) mod sandbox;
#[cfg(test)]
mod sandbox_browser_tests;
#[cfg(test)]
mod sandbox_tests;
pub(crate) mod subagents;
#[cfg(test)]
mod subagents_tests;
#[cfg(test)]
mod tests;
pub(crate) mod tls_roots;
pub(crate) mod tools;
#[cfg(test)]
mod tools_tests;

pub use codex::{GUARD_EXE, codex_guard_program, codex_hook_config, codex_hook_config_with_home};

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Claude Code's `settingSources` for every Clauseo session: the user settings only. For a
/// Claude agent with a kind, `CLAUDE_CONFIG_DIR` is forced to Clauseo's own home for the agent
/// (R20 P4a), so "user" settings are that home's `settings.json`, not the lawyer's
/// `~/.claude`; an agent with no kind reads the user's own. Claude Code then reads none
/// of the matter folder's project and local settings: not its `.mcp.json`, hooks or permission
/// rules. claude-agent-acp reads those files itself, whatever `settingSources` says, and takes a
/// starting mode from them: the SDK drops `acceptEdits` only from the project file, so a
/// folder's `.claude/settings.local.json` can still start the session in accept-edits
/// (clauseo-acp-private#218, older than this setting). A live run on the Mac mini showed `[]`
/// breaking the login (2026-11-02). Plugins in that user settings folder still load their skills,
/// hooks and agents under `["user"]`; strict MCP config is what keeps their MCP servers out.
/// `tests/scenarios/mcp-defaults.jsonl` pins this value.
pub(crate) const CLAUDE_SETTING_SOURCES: &[&str] = &["user"];

/// `settingSources` when the firm's `managed-mcp.json` is present ([`McpControl::Managed`]):
/// the same user settings, for the login. Strict MCP config cannot go there. A file that parses
/// takes exclusive control: user-scope and plugin servers never load, even with `["user"]`
/// (CLI 2.1.293, read from its source: research-claude.md, "Follow-up: managed paths"; not run
/// live). claude.ai connectors stay off too, since [`CLAUDE_AI_MCP_VAR`] is still `false`.
pub(crate) const CLAUDE_MANAGED_SETTING_SOURCES: &[&str] = &["user"];

/// The permission mode Clauseo starts every Claude session in outside forced ask: Claude Code's
/// `auto` (Rohan, 2026-11-11: a first read of a matter's papers never asks; decision records
/// 0025 and 0030). Claude Code's classifier then reviews each tool call that would ask in
/// `default` and lets the safe ones through; deny rules still come first, so the data fence's
/// (`data_fence.rs`) still refuse, and the guard's `PreToolUse` hook still runs before each
/// write. Undo per turn is the lawyer's way back. When auto is not available (the agent offers
/// none, refuses the switch, or moves off it at once: the CLI's account or model gate),
/// [`CLAUDE_FALLBACK_MODE`] runs instead, with one notice (`actor/turn/session/claude_mode.rs`).
/// A forced-ask session starts in [`forced_ask::ASK_MODE`](super::forced_ask::ASK_MODE)
/// instead ([`claude_mode`]).
///
/// The window cannot pick another mode yet (the desktop bridge refuses `SetMode`), so this is
/// Clauseo's choice at every session start. Its cost: a mode the lawyer reached inside a session
/// goes back to this one when the session is next loaded. Re-applying the thread's last logged
/// mode instead cannot tell such a choice from a mode the user's settings started a session in:
/// both are logged as `ModeChanged`.
///
/// claude-agent-acp 0.110.1 starts a session in the mode it reads itself: an explicit one (only its
/// own re-creations pass one), else a resumed transcript's, else `permissions.defaultMode` from the
/// user's, the folder's and managed settings, else `auto` when it lists it
/// (`v1/acp-agent.js:7580-7588`, `session-mode.js:29-56`). Its `SettingsManager` calls the SDK's
/// `resolveSettings({cwd})`, which never sees the flag settings (`settings.js:94`), and it forces
/// the SDK option `permissionMode` over `_meta.claudeCode.options` (`v1/acp-agent.js:7729`, after
/// the options' spread at `:7713`). So neither `_meta` route can set it: the host sends
/// `session/set_mode` with this mode before the first prompt when the session started in another
/// (`actor/turn/session.rs`), and [`session_meta`] takes bypass off the table.
pub(crate) const CLAUDE_MODE: &str = "auto";

/// The mode a Claude session outside forced ask runs in when [`CLAUDE_MODE`] is not available:
/// `acceptEdits`. Claude Code 2.1.293 then approves, unasked, `Edit`, `Write` and
/// `NotebookEdit` and the Bash commands `mkdir`, `touch`, `rm`, `rmdir`, `mv`, `cp` and `sed`
/// whose paths are inside the working directory or `additionalDirectories`; everything else
/// asks as in `default`. claude-agent-acp 0.110.1 falls back to the same mode itself when the
/// model cannot run auto (`session-mode.js:83-89`; a session that started in `auto` only
/// because no `defaultMode` was set goes to `default` instead, `:76-82`).
pub(crate) const CLAUDE_FALLBACK_MODE: &str = "acceptEdits";

/// The mode a Claude session starts in and is switched to before its first prompt:
/// [`CLAUDE_MODE`], or `default` in forced ask
/// ([`forced_ask::ASK_MODE`](super::forced_ask::ASK_MODE)), where Claude asks before every
/// change. Never `bypassPermissions`.
pub(crate) fn claude_mode(forced_ask: bool) -> &'static str {
    if forced_ask {
        super::forced_ask::ASK_MODE
    } else {
        CLAUDE_MODE
    }
}

/// The variable that, set to `false`, keeps claude.ai's connectors out of a Claude Code session.
pub(crate) const CLAUDE_AI_MCP_VAR: &str = "ENABLE_CLAUDEAI_MCP_SERVERS";

/// The Claude Code variable that, set, puts its `Bash` and `PowerShell` back in the session's
/// folder after every call (Claude Code 2.1.293's `SBn()`, read from the process env on each
/// call). Clauseo sets it to `1`, so every shell call starts in the matter's folder, which is
/// where `crate::shell_writes` joins a call's relative targets: a `cd` in one call never moves
/// where the next writes. Unset, the folder a call's `cd` reaches would carry to the next.
pub(crate) const CLAUDE_SHELL_FOLDER_VAR: &str = "CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR";

/// A launch variable Clauseo sets on every Claude launch ([`CLAUDE_LAUNCH_VARS`]).
pub(crate) const CLAUDE_CRON_OFF_VAR: &str = "CLAUDE_CODE_DISABLE_CRON";

/// A launch variable Clauseo sets on every Claude launch ([`CLAUDE_LAUNCH_VARS`]).
pub(crate) const CLAUDE_GIT_STATUS_OFF_VAR: &str = "CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS";

/// A flag setting Clauseo sets on every Claude launch ([`claude_flag_settings`]).
pub(crate) const GIT_STATUS_SETTING: &str = "includeGitInstructions";

/// A launch variable Clauseo sets on every Claude launch ([`CLAUDE_LAUNCH_VARS`]).
pub(crate) const CLAUDE_MESSAGING_OFF_VAR: &str = "CLAUDE_CODE_HARBOR_KITE";

/// A launch variable Clauseo sets on every Claude launch ([`CLAUDE_LAUNCH_VARS`]).
pub(crate) const CLAUDE_MARKETPLACE_OFF_VAR: &str =
    "CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL";

/// A launch variable Clauseo sets to `1` on every Claude launch, every login
/// ([`CLAUDE_LAUNCH_VARS`]): Claude Code offers none of its own agent types (general-purpose,
/// Explore, Plan, web-fetch), so the lead can launch only the helpers Clauseo defines
/// ([`subagents`]). Claude Code 2.1.x binary (2.1.293 and 2.1.330), search text
/// `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS&&`: the built-in list is empty when this variable is
/// on and the session is not interactive, which every Agent SDK session is (stdout is a pipe).
/// It is read as a yes/no (`1`, `true`, `yes`, `on` count as on). It also gives the lead its
/// own `WebFetch` back: the lead's tool list drops `WebFetch` only while the web-fetch agent
/// stands in for it, which needs the agent mode `default`, and this variable makes it `none`
/// (search text `return n&&dfe()==="default"` in 2.1.293, `return o&&y0e()==="default"` in
/// 2.1.330).
pub(crate) const CLAUDE_BUILTIN_AGENTS_OFF_VAR: &str = "CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS";

/// A launch variable Clauseo sets to `0` on every Claude launch, every login
/// ([`CLAUDE_LAUNCH_VARS`]): the web-fetch agent stays off whatever Anthropic's flag
/// `tengu_clever_orbit` says, so the lead keeps its own `WebFetch` tool instead of launching a
/// helper on its own model for each page. Claude Code 2.1.x binary, search text
/// `enabled??=a.CLAUDE_CODE_WEB_FETCH_AGENT??` (2.1.293; `s.` in 2.1.330): a value the variable
/// gives wins over the flag; `0`, `false`, `no` and `off` give off, anything else gives no value.
pub(crate) const CLAUDE_WEB_FETCH_AGENT_VAR: &str = "CLAUDE_CODE_WEB_FETCH_AGENT";

/// A launch variable Clauseo sets on every Claude launch, every login ([`CLAUDE_LAUNCH_VARS`]):
/// at most [`CLAUDE_HELPERS_AT_ONCE`] helpers at once, as the instructions ask ("4 to 6 at
/// once"). Claude Code 2.1.x binary, search text `Concurrent subagent limit reached`: a launch
/// past the cap is refused with that message; unset, the cap is 20. Read as a whole number of
/// at least 1, digits only.
pub(crate) const CLAUDE_HELPERS_AT_ONCE_VAR: &str = "CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS";

/// The most helpers a Claude session runs at once ([`CLAUDE_HELPERS_AT_ONCE_VAR`]).
pub(crate) const CLAUDE_HELPERS_AT_ONCE: &str = "6";

/// A launch variable Clauseo sets to `0` on every Claude launch, every login
/// ([`CLAUDE_LAUNCH_VARS`]): no fork, the `Agent` tool's `subagent_type: "fork"` that copies the
/// lead with its whole conversation and runs on the lead's own model. The built-in switch
/// ([`CLAUDE_BUILTIN_AGENTS_OFF_VAR`]) leaves it on: a live run on Claude Code 2.1.330 offered
/// "paper-reader, fork". Claude Code 2.1.x binary, search text
/// `if(s.CLAUDE_CODE_FORK_SUBAGENT===!1)return"disabled"` (2.1.330; `a.` in 2.1.293): read as a
/// yes/no/unset, `0` turns it off before Anthropic's flag is read. The same switch decides
/// whether helpers are forced to start in the background (search text `function bZt()`).
pub(crate) const CLAUDE_FORK_OFF_VAR: &str = "CLAUDE_CODE_FORK_SUBAGENT";

// A place for further launch variables of this kind: each goes in `CLAUDE_LAUNCH_VARS` below
// and in the limits reader's and sign-in's `CLAUDE_FORCED_VARS`, with a test in
// `tools_tests.rs`. A variable that depends on the login goes in
// `subagents::helper_model_vars` instead.

/// The variables every Claude launch sets, each in the adapter's own environment
/// ([`claude_launch_vars`]), in `claudeCode.options.env` and in the flag settings' `env`
/// ([`session_meta`]); the limits reader sets the same (`clauseo_limits`' `CLAUDE_FORCED_VARS`).
pub(crate) const CLAUDE_LAUNCH_VARS: [(&str, &str); 8] = [
    (CLAUDE_CRON_OFF_VAR, "1"),
    (CLAUDE_GIT_STATUS_OFF_VAR, "1"),
    (CLAUDE_MESSAGING_OFF_VAR, "0"),
    (CLAUDE_MARKETPLACE_OFF_VAR, "1"),
    (CLAUDE_BUILTIN_AGENTS_OFF_VAR, "1"),
    (CLAUDE_WEB_FETCH_AGENT_VAR, "0"),
    (CLAUDE_HELPERS_AT_ONCE_VAR, CLAUDE_HELPERS_AT_ONCE),
    (CLAUDE_FORK_OFF_VAR, "0"),
];

/// A flag setting Clauseo sets on every Claude launch ([`claude_flag_settings`]).
pub(crate) const CROSS_SESSION_SETTING: &str = "crossSessionInbound";

/// A flag setting Clauseo sets on every Claude launch ([`claude_flag_settings`]).
pub(crate) const REMOTE_CONTROL_OFF_SETTING: &str = "disableRemoteControl";

/// The keys every Claude launch sets in its flag settings ([`session_meta`]), over a hook's.
pub(crate) fn claude_flag_settings() -> [(&'static str, Value); 3] {
    [
        (GIT_STATUS_SETTING, Value::Bool(false)),
        (CROSS_SESSION_SETTING, Value::from("refuse")),
        (REMOTE_CONTROL_OFF_SETTING, Value::Bool(true)),
    ]
}

/// The variables every Claude launch sets in its own environment, over any of the same name
/// from the settings, the hook or the inherited environment: [`CLAUDE_LAUNCH_VARS`].
pub(crate) fn claude_launch_vars() -> BTreeMap<OsString, OsString> {
    CLAUDE_LAUNCH_VARS
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

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
/// - [`CLAUDE_SHELL_FOLDER_VAR`] and each of [`CLAUDE_LAUNCH_VARS`], in
///   `claudeCode.options.env` (claude-agent-acp 0.110.1 spreads it over its own env for the
///   SDK's spawn of Claude Code, `v1/acp-agent.js:7659-7669`) and, with `keys` (a Claude agent),
///   in `claudeCode.options.settings.env` (flag settings, over a user's or project's), with
///   [`claude_flag_settings`] beside it. A hook value at `settings` or its `env` that is
///   not an object is then replaced.
/// - On Windows, with `keys`: `CLAUDE_CODE_USE_POWERSHELL_TOOL` `"1"` in both of those `env`
///   objects, over any hook value, and no `CLAUDE_CODE_GIT_BASH_PATH` (`agent/one_shell.rs`).
/// - `claudeCode.options.allowDangerouslySkipPermissions`: `false`. claude-agent-acp 0.110.1 then
///   offers no bypass mode, nor the plan-exit options that switch to it, and never starts a
///   session in it: a settings `defaultMode` of `bypassPermissions` becomes `default`, and a
///   resumed transcript's `bypassPermissions` is rejected, falling back to the settings'
///   `defaultMode`, which may be `acceptEdits` (`v1/acp-agent.js:7580-7588`,
///   `session-mode.js:29-56`, `permissions/modes.js` `resolvePermissionMode`). Whatever mode it
///   then starts in (`default`, `auto`, `plan`, or `acceptEdits` in forced ask) is switched to
///   [`claude_mode`] after the start, since `_meta` cannot set it.
/// - `claudeCode.options.extraArgs`: `setting-sources`, [`MODE_FLAGS`] and [`TOOL_FLAGS`] taken
///   out, in both forms the SDK turns into a flag (`{"setting-sources": "…"}` and
///   `{"setting-sources=…": null}`), since an extra flag goes after the SDK's own and would win.
/// - Under [`McpControl::Clauseo`]: `strictMcpConfig: true` and `settingSources`
///   [`CLAUDE_SETTING_SOURCES`].
/// - Under [`McpControl::Managed`]: `strictMcpConfig: false`, `settingSources`
///   [`CLAUDE_MANAGED_SETTING_SOURCES`], and no `mcp-config` or `strict-mcp-config` in
///   `extraArgs` (either makes Claude Code refuse to start there).
/// - In every mode: no `mcpServers`. A hook cannot add servers through `_meta`.
/// - With `keys` (a Claude agent): [`profile::claude_options`], before the guard's entry, so
///   the guard's `hooks` and `disableAllHooks` go over whatever settings it merged; then
///   [`subagents::subagent_meta`], which settles `agents` and the append's last part, and
///   [`subagents::helper_model_env`], which puts every helper on Haiku on Anthropic's own login
///   (in `claudeCode.options.env` and the flag settings' `env`, over a hook's values).
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
    env.insert(CLAUDE_SHELL_FOLDER_VAR.to_owned(), Value::from("1"));
    super::one_shell::meta_env(&mut env, keys.is_some() && cfg!(windows));
    for (name, value) in CLAUDE_LAUNCH_VARS {
        env.insert(name.to_owned(), Value::from(value));
    }
    options.insert("env".to_owned(), Value::Object(env));
    options.insert(ALLOW_BYPASS_OPTION.to_owned(), Value::Bool(false));
    let (strict, sources) = match control {
        McpControl::Clauseo => (true, CLAUDE_SETTING_SOURCES),
        McpControl::Managed => (false, CLAUDE_MANAGED_SETTING_SOURCES),
    };
    if let Some(Value::Object(extra)) = options.get_mut("extraArgs") {
        extra.retain(|flag, _| {
            let taken_out = is_flag(flag, SETTING_SOURCES_FLAG)
                || MODE_FLAGS.iter().any(|name| is_flag(flag, name))
                || TOOL_FLAGS.iter().any(|name| is_flag(flag, name))
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
        subagents::subagent_meta(&mut meta, &mut options, keys.login, keys.web_search.on);
        subagents::helper_model_env(&mut options, keys.login);
    }
    if let Some(entry) = guard.and_then(guard_entry) {
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
    if keys.is_some() {
        // Claude Code applies a settings `env` over the process env, so the variable goes in
        // the flag settings too: a user's or project's settings `env` cannot undo it.
        let mut settings = take_object(&mut options, "settings");
        let mut settings_env = take_object(&mut settings, "env");
        settings_env.insert(CLAUDE_SHELL_FOLDER_VAR.to_owned(), Value::from("1"));
        super::one_shell::meta_env(&mut settings_env, cfg!(windows));
        for (name, value) in CLAUDE_LAUNCH_VARS {
            settings_env.insert(name.to_owned(), Value::from(value));
        }
        settings.insert("env".to_owned(), Value::Object(settings_env));
        for (key, value) in claude_flag_settings() {
            settings.insert(key.to_owned(), value);
        }
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

/// The claude-agent-acp option that, `false`, keeps `bypassPermissions` out of a session.
pub(crate) const ALLOW_BYPASS_OPTION: &str = "allowDangerouslySkipPermissions";

/// The Claude Code flag `settingSources` becomes.
const SETTING_SOURCES_FLAG: &str = "setting-sources";

/// The `extraArgs` flags that would set the tools or the flag settings beside
/// `claudeCode.options`' own keys (an extra flag goes after the SDK's own and would win).
const TOOL_FLAGS: [&str; 6] = [
    "tools",
    "allowedTools",
    "allowed-tools",
    "disallowedTools",
    "disallowed-tools",
    "settings",
];

/// The Claude Code flags that set the permission mode or allow bypass. An extra flag goes after
/// the SDK's own `--permission-mode`, so it would win over [`CLAUDE_MODE`] and over
/// [`ALLOW_BYPASS_OPTION`].
const MODE_FLAGS: [&str; 3] = [
    "permission-mode",
    "dangerously-skip-permissions",
    "allow-dangerously-skip-permissions",
];

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
    /// Forced ask (`agent/forced_ask.rs`): the hook runs before every tool, with
    /// [`ASK_ALL_FLAG`](crate::agent::forced_ask::ASK_ALL_FLAG) after the folder.
    pub(crate) ask_all: bool,
}

/// The tools the pre-write guard's hook runs before: Claude Code's file tools (`MultiEdit` is
/// only a legacy alias at 2.1.293, harmless to keep), `Bash` and `PowerShell` (on by default on
/// Windows for some accounts), which the guard checks by a heuristic. A matcher of names joined
/// by `|` is an exact list.
pub(crate) const GUARD_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit|Bash|PowerShell";

/// The seconds Claude Code waits for the guard before going on without it (default 600).
pub(crate) const GUARD_TIMEOUT_SECONDS: u64 = 10;

/// The `PreToolUse` entry that runs `program` with [`pre_write_guard::FLAG`]
/// (`crate::pre_write_guard`) and `--matter <matter>` before each of [`GUARD_MATCHER`]'s tools;
/// with `ask_all`, before every tool (MCP tools included) and with `--ask-all` last.
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
pub(crate) fn guard_entry(guard: GuardHook<'_>) -> Option<Value> {
    let GuardHook {
        program,
        matter,
        ask_all,
    } = guard;
    let (Some(command), Some(matter)) = (program.to_str(), matter.to_str()) else {
        tracing::warn!(
            "the pre-write guard's program or matter path is not UTF-8: no guard hook goes to \
             the agent"
        );
        return None;
    };
    let mut args = vec![
        crate::pre_write_guard::FLAG,
        crate::pre_write_guard::MATTER_FLAG,
        matter,
    ];
    let matcher = if ask_all {
        args.push(crate::agent::forced_ask::ASK_ALL_FLAG);
        crate::agent::forced_ask::EVERY_TOOL
    } else {
        GUARD_MATCHER
    };
    Some(serde_json::json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": command,
            "args": args,
            "timeout": GUARD_TIMEOUT_SECONDS,
        }],
    }))
}

/// Why `guard` gives no [`guard_entry`], as a kind fit for a log line (no path): `no_program`
/// when the host has no guard program (`HostConfig::with_pre_write_guard` unset, as when
/// `current_exe` failed), `program_not_utf8` or `matter_not_utf8`. `None` when it gives one.
pub(crate) fn guard_missing(guard: Option<GuardHook<'_>>) -> Option<&'static str> {
    match guard {
        None => Some("no_program"),
        Some(guard) if guard.program.to_str().is_none() => Some("program_not_utf8"),
        Some(guard) if guard.matter.to_str().is_none() => Some("matter_not_utf8"),
        Some(_) => None,
    }
}

/// The object at `key`, taken out of `map`; empty when it is missing or not an object.
pub(crate) fn take_object(map: &mut Map<String, Value>, key: &str) -> Map<String, Value> {
    match map.remove(key) {
        Some(Value::Object(inner)) => inner,
        _ => Map::new(),
    }
}

/// The variables for `launch_spec`'s hook slot: the hook's, then [`CLAUDE_AI_MCP_VAR`] as
/// `false`, replacing any hook variable of that name in any case.
pub(crate) fn launch_env(mut hook: BTreeMap<OsString, OsString>) -> BTreeMap<OsString, OsString> {
    hook.retain(|name, _| !name.eq_ignore_ascii_case(CLAUDE_AI_MCP_VAR));
    hook.insert(OsString::from(CLAUDE_AI_MCP_VAR), OsString::from("false"));
    hook
}

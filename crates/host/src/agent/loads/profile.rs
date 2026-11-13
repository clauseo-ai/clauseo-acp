//! The host's half of the launch profile (R21, R22; decision record 0030): what it adds after
//! the profile hook, per agent kind, from the settings and the session.
//!
//! - Claude ([`claude_options`]): `disallowedTools` always holds `WebSearch` and `WebFetch`
//!   when `tools.web_search` is off, whatever the hook sent; when it is on, `allowedTools` gains
//!   `WebSearch` (not on a Bedrock login, which has none), a bare `WebFetch` (every page fetched
//!   unasked) and one `WebFetch(domain:…)` per seed site, and `mcp__case-law__<tool>` per
//!   search and read tool (`case_law::UNASKED_TOOLS`) only in a session that got the
//!   `case-law` entry (any hook rule for that server is dropped otherwise), and before it
//!   `mcp__<name>` for each of the lawyer's servers that runs without asking
//!   (`agent/mcp_trust.rs`); in forced ask (`agent/forced_ask.rs`) there is no `allowedTools`
//!   at all, the hook's included. `additionalDirectories` gains the work folder and the matter
//!   notes folder, after any hook entries. `settings` gains `disableSkillShellExecution: true`
//!   always, and `skipWebFetchPreflight: true` on a Bedrock,
//!   Vertex or Foundry login, and always `permissions.deny` gains the data folder's rules
//!   (`agent/data_fence.rs`), after any the hook put there; in forced ask it also gains the
//!   `permissions.ask` rules and `sandbox.autoAllowBashIfSandboxed: false`, over Claude Code's
//!   own sandbox, which the settings gain before it on macOS and Linux (`agent/loads/sandbox.rs`;
//!   none on Windows). These are flag
//!   settings: they outrank the user's settings, and Claude Code merges every source's
//!   `permissions.deny`, so no user rule takes one out.
//! - Codex ([`codex_keys`]): of the hook's keys only `developer_instructions`
//!   (`agent::codex::PROFILE_KEYS`), its tokens filled; then the host's `web_search` (`live` or
//!   `disabled`) and both folders in `sandbox_workspace_write.writable_roots`.
//!   `agent::codex::config` merges them add-only before R15's `CODEX_OVERRIDES`, so R15's pins
//!   win.
//! - Scope: the tokens are filled only in what the profile supplied (its instructions, its
//!   `systemPrompt.append`, its `browser` entry), never in the lawyer's own MCP servers or a
//!   settings agent's `env`.
//! - Both: the instructions' tokens ([`fill_instructions`]). An unfillable token fails the
//!   launch with `agent.launch_failed`, naming the token.
//!
//! An agent with no kind gets none of this. Every value is filled into a decoded string and
//! then encoded by `serde_json`, so quotes, backslashes and newlines in a path are escaped for
//! JSON, and a folder whose name holds a token stays literal.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use clauseo_profile::placeholders::{
    self, BROWSER, CONNECTORS, IN_INSTRUCTIONS, MATTER_NOTES, WORK_FOLDER,
};
use clauseo_protocol::{
    AgentKind, ClauseoTool, ErrorCode, ToolAvailability, ToolState, ToolsUnavailableReason,
};
use serde_json::{Map, Value};

use super::browser::Switch;
use super::take_object;
use crate::error::Error;

/// Claude Code's built-in web tools, which `tools.web_search` turns off.
pub(crate) const WEB_TOOLS: [&str; 2] = ["WebSearch", "WebFetch"];

/// The variables that put Claude Code on a cloud provider's login.
pub(crate) const CLOUD_LOGIN_VARS: [&str; 3] = [
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];

/// Which login Claude Code runs on, as the launch env says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Login {
    /// Anthropic's own.
    FirstParty,
    /// Amazon Bedrock: no `WebSearch` (the server tool is not there), `WebFetch` works.
    Bedrock,
    /// Vertex or Foundry.
    OtherCloud,
}

impl Login {
    /// From the merged launch env (`set` gives a variable's value, the hook's over the
    /// settings' over the inherited): a cloud variable counts when non-empty and not `0`.
    pub(crate) fn from_env(set: impl Fn(&str) -> Option<OsString>) -> Self {
        let on = |name: &str| set(name).is_some_and(|value| !value.is_empty() && value != "0");
        if on(CLOUD_LOGIN_VARS[0]) {
            Login::Bedrock
        } else if CLOUD_LOGIN_VARS[1..].iter().any(|name| on(name)) {
            Login::OtherCloud
        } else {
            Login::FirstParty
        }
    }

    /// The merged launch env's view for [`Login::from_env`]: `hook`, then `settings`, then
    /// `inherited`, each name matched ignoring ASCII case.
    pub(crate) fn of(
        hook: &BTreeMap<OsString, OsString>,
        settings: &BTreeMap<String, String>,
        inherited: impl Fn(&str) -> Option<OsString>,
    ) -> Self {
        Login::from_env(|name| {
            hook.iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.clone())
                .or_else(|| {
                    settings
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(name))
                        .map(|(_, value)| OsString::from(value))
                })
                .or_else(|| inherited(name))
        })
    }
}

/// The session's two folders, as UTF-8 text for JSON, and what `{connectors}` becomes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Folders<'a> {
    /// `{work_folder}`, `<data>/run/<session id>/work`.
    pub(crate) work: &'a str,
    /// `{matter_notes}`, `<data>/agent-state/<matter id>/notes`.
    pub(crate) notes: &'a str,
    /// `{connectors}` (R24, decision D6): not a folder, but the session's like them. The
    /// connectors paragraph in a session that got the `case-law` entry
    /// (`clauseo_profile::instructions::connectors_part`), else the sentence that says there is no
    /// case-law database, which [`Folders::of`] starts with: a caller that never says gets that.
    pub(crate) connectors: &'a str,
    /// `{browser}`: the browser part (`clauseo_profile::instructions::browser_part`), the
    /// `agent-browser` rules in a session with the shell tool, else the sentence that says it
    /// has no browser, which [`Folders::of`] starts with.
    pub(crate) browser: &'a str,
}

impl<'a> Folders<'a> {
    /// Both folders as text, or `agent.launch_failed` naming the token whose folder is not
    /// UTF-8 (JSON cannot carry it, and a lossy copy would name another folder).
    pub(crate) fn of(work: &'a Path, notes: &'a Path) -> Result<Self, Error> {
        let text = |path: &'a Path, token: &str| {
            path.to_str().ok_or_else(|| {
                launch_failed(&format!(
                    "the folder for {token} is not UTF-8: {}",
                    path.display()
                ))
            })
        };
        Ok(Self {
            work: text(work, WORK_FOLDER)?,
            notes: text(notes, MATTER_NOTES)?,
            connectors: clauseo_profile::instructions::connectors_part(false),
            browser: clauseo_profile::instructions::browser_part(
                clauseo_profile::instructions::BrowserPart::Off,
            ),
        })
    }

    /// The same folders, with `{browser}` filled by `browser`.
    pub(crate) fn with_browser(self, browser: &'a str) -> Self {
        Self { browser, ..self }
    }

    /// The same folders, with `{connectors}` filled by `connectors`.
    pub(crate) fn with_connectors(self, connectors: &'a str) -> Self {
        Self { connectors, ..self }
    }
}

/// What the host adds to a Claude agent's `_meta`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClaudeKeys<'a> {
    /// `tools.web_search`.
    pub(crate) web_search: Switch,
    /// The login.
    pub(crate) login: Login,
    /// The session's folders.
    pub(crate) folders: Folders<'a>,
    /// The hosts seeded as `WebFetch(domain:…)` rules, beside the bare `WebFetch` that already
    /// covers them: kept as the record of the trusted sites (R21's list).
    pub(crate) seed_sites: &'a [&'a str],
    /// The session runs in forced ask (`agent/forced_ask.rs`): no `allowedTools` at all, the
    /// hook's included, and the forced-ask `permissions.ask` rules in the flag settings.
    pub(crate) forced_ask: bool,
    /// The data folder's deny rules (`RunFolders::data_deny`).
    pub(crate) data: &'a crate::agent::data_fence::LaunchDeny,
    /// The session gets the `case-law` entry (R24): `allowedTools` gains
    /// `mcp__case-law__<tool>` per `case_law::UNASKED_TOOLS` (decision D7), never `recharge`.
    /// Without it, any hook rule for a `case-law` server is dropped.
    pub(crate) case_law: bool,
    /// The session has the browser as a shell tool: `allowedTools` gains agent-browser's page
    /// subcommands, `disallowedTools` its denied flags, subcommands and file or JavaScript
    /// commands, and `permissions.ask` the options that still ask
    /// (`clauseo_profile::claude::agent_browser_allowed`, `agent_browser_denied`,
    /// `agent_browser_ask`). Without it, any hook rule naming agent-browser is dropped, so a
    /// session without the browser has none.
    pub(crate) browser: bool,
    /// Claude Code's own sandbox (`agent::loads::sandbox`), on macOS and Linux; none on
    /// Windows, where commands keep asking.
    pub(crate) sandbox: Option<&'a super::sandbox::ClaudeSandbox>,
    /// The matter folder: Claude's `Edit` is denied its `.clauseo` folder (`matter.json`, the
    /// marker that finds a moved matter folder again). None in a test that is about other keys.
    pub(crate) matter: Option<&'a Path>,
    /// The lawyer's servers whose tools run without asking (`agent/mcp_trust.rs`):
    /// `allowedTools` gains `mcp__<name>` for each, before case law's. None go in forced ask,
    /// where there is no `allowedTools` at all.
    pub(crate) trusted_servers: &'a [String],
}

/// Adds [`ClaudeKeys`] to `claudeCode.options` (see the module doc).
pub(crate) fn claude_options(options: &mut Map<String, Value>, keys: &ClaudeKeys<'_>) {
    let mut disallowed = take_array(options, "disallowedTools");
    let mut allowed = take_array(options, "allowedTools");
    if keys.web_search.on {
        if keys.login != Login::Bedrock {
            push_new(&mut allowed, "WebSearch");
        }
        push_new(&mut allowed, "WebFetch");
        for site in keys.seed_sites {
            push_new(&mut allowed, &format!("WebFetch(domain:{site})"));
        }
    } else {
        for tool in WEB_TOOLS {
            push_new(&mut disallowed, tool);
        }
        allowed.retain(|rule| !rule.as_str().is_some_and(is_web_rule));
    }
    allowed.retain(|rule| !rule.as_str().is_some_and(super::case_law::is_case_law_rule));
    for server in keys.trusted_servers {
        push_new(&mut allowed, &crate::agent::mcp_trust::claude_rule(server));
    }
    if keys.case_law {
        for rule in super::case_law::claude_allow() {
            push_new(&mut allowed, &rule);
        }
    }
    let names_browser = |rule: &Value| rule.as_str().is_some_and(|r| r.contains("agent-browser"));
    allowed.retain(|rule| !names_browser(rule));
    disallowed.retain(|rule| !names_browser(rule));
    if keys.browser {
        allowed.extend(clauseo_profile::claude::agent_browser_allowed().map(Value::String));
        disallowed.extend(clauseo_profile::claude::agent_browser_denied().map(Value::String));
    }
    if !disallowed.is_empty() {
        options.insert("disallowedTools".to_owned(), Value::Array(disallowed));
    }
    if !keys.forced_ask {
        options.insert("allowedTools".to_owned(), Value::Array(allowed));
    }
    let mut directories = take_array(options, "additionalDirectories");
    push_new(&mut directories, keys.folders.work);
    push_new(&mut directories, keys.folders.notes);
    options.insert(
        "additionalDirectories".to_owned(),
        Value::Array(directories),
    );
    let mut settings = take_object(options, "settings");
    settings.insert("disableSkillShellExecution".to_owned(), Value::Bool(true));
    if keys.login != Login::FirstParty {
        settings.insert("skipWebFetchPreflight".to_owned(), Value::Bool(true));
    }
    deny_data_folder(&mut settings, keys.data);
    if keys.browser {
        browser_ask_settings(&mut settings);
    }
    if let Some(matter) = keys.matter {
        deny_matter_marker(&mut settings, matter);
    }
    // Before forced ask, whose `autoAllowBashIfSandboxed: false` must win.
    if let Some(sandbox) = keys.sandbox {
        super::sandbox::sandbox_settings(&mut settings, sandbox);
    }
    if keys.forced_ask {
        forced_ask_settings(&mut settings);
    }
    options.insert("settings".to_owned(), Value::Object(settings));
}

/// The size past which a launch's settings shed the data folder's variable forms from its shell
/// rules (`data_fence::windows_shell_deny_rules_without_variables`): about 1.8 MiB, under the
/// 2 MiB settings file Claude Code reads (`agent::launch_settings`), which refuses a larger one.
pub(crate) const SHED_AT: usize = 1_887_436;

/// `permissions.deny` gains the data folder's rules (`agent/data_fence.rs`; on Windows also its
/// `Bash` and `PowerShell` rules, `data_fence::windows_shell_deny_rules`, without the variable
/// forms when all of them would take the settings past [`SHED_AT`]), and
/// `permissions.allow` the Read of the built-in skills' folder, each after any the hook put
/// there. A hook value at `permissions`, `deny` or `allow` of another type is replaced. With no
/// spelling (a unit test's keys) nothing changes.
fn deny_data_folder(
    settings: &mut Map<String, Value>,
    data: &crate::agent::data_fence::LaunchDeny,
) {
    if data.spellings.is_empty() {
        return;
    }
    let mut permissions = take_object(settings, "permissions");
    let mut deny = take_array(&mut permissions, "deny");
    for rule in crate::agent::data_fence::claude_deny_rules(data) {
        push_new(&mut deny, &rule);
    }
    if cfg!(windows) {
        let var = |name: &str| std::env::var_os(name);
        let spent = json_len(&*settings) + json_len(&deny);
        let shell = shell_rules_within(data, &var, spent, SHED_AT);
        // A set, not `push_new`: a busy data folder gives a few thousand of these.
        let mut known: std::collections::HashSet<String> = deny
            .iter()
            .filter_map(|rule| rule.as_str().map(str::to_owned))
            .collect();
        for rule in shell {
            if known.insert(rule.clone()) {
                deny.push(Value::String(rule));
            }
        }
    }
    permissions.insert("deny".to_owned(), Value::Array(deny));
    let mut allow = take_array(&mut permissions, "allow");
    for rule in crate::agent::data_fence::claude_allow_rules(data) {
        push_new(&mut allow, &rule);
    }
    permissions.insert("allow".to_owned(), Value::Array(allow));
    settings.insert("permissions".to_owned(), Value::Object(permissions));
}

/// The data folder's Windows shell rules for a launch whose other settings take `spent` bytes as
/// JSON: all of them, or without the variable forms
/// (`data_fence::windows_shell_deny_rules_without_variables`) when all of them would take the
/// settings past `budget` ([`SHED_AT`]), logged with fixed fields only.
pub(crate) fn shell_rules_within(
    data: &crate::agent::data_fence::LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
    spent: usize,
    budget: usize,
) -> Vec<String> {
    let shell = crate::agent::data_fence::windows_shell_deny_rules(data, var);
    let total = spent + json_len(&shell);
    if total <= budget {
        return shell;
    }
    tracing::warn!(
        settings_bytes = total,
        budget,
        shed = "vars",
        "Claude's settings outgrew their budget; the data folder's variable forms are shed"
    );
    crate::agent::data_fence::windows_shell_deny_rules_without_variables(data, var)
}

/// `value`'s length as JSON (0 if it can't be written, which a map of JSON values never is).
fn json_len(value: &impl serde::Serialize) -> usize {
    serde_json::to_string(value).map_or(0, |text| text.len())
}

/// `permissions.ask` gains agent-browser's allowed commands that still ask
/// (`clauseo_profile::claude::agent_browser_ask`: a `file://` URL, an option that loads page
/// JavaScript, `-p`), after any the hook put there; the commands that read or write a local file
/// or run page JavaScript are denied instead (`agent_browser_denied`). A hook value at
/// `permissions` or `ask` of another type is replaced.
fn browser_ask_settings(settings: &mut Map<String, Value>) {
    let mut permissions = take_object(settings, "permissions");
    let mut ask = take_array(&mut permissions, "ask");
    for rule in clauseo_profile::claude::agent_browser_ask() {
        push_new(&mut ask, &rule);
    }
    permissions.insert("ask".to_owned(), Value::Array(ask));
    settings.insert("permissions".to_owned(), Value::Object(permissions));
}

/// `permissions.deny` gains `Edit` of the matter's `.clauseo` folder and all in it, after any
/// rules already there. A matter folder that is not UTF-8 gets none (no rule could name it).
fn deny_matter_marker(settings: &mut Map<String, Value>, matter: &Path) {
    let Some(text) = matter.to_str() else {
        return;
    };
    let base = crate::agent::data_fence::rule_path(text, cfg!(windows));
    let marker = super::sandbox::MATTER_MARKER;
    let mut permissions = take_object(settings, "permissions");
    let mut deny = take_array(&mut permissions, "deny");
    for rule in [
        format!("Edit({base}/{marker})"),
        format!("Edit({base}/{marker}/**)"),
    ] {
        push_new(&mut deny, &rule);
    }
    permissions.insert("deny".to_owned(), Value::Array(deny));
    settings.insert("permissions".to_owned(), Value::Object(permissions));
}

/// Forced ask's second fence in the flag settings (`agent/forced_ask.rs` [`ASK_RULES`]):
/// `permissions.ask` gains every rule, after any the hook put there, and
/// `sandbox.autoAllowBashIfSandboxed` is `false`. A hook value at `permissions`, `ask` or
/// `sandbox` of another type is replaced.
///
/// [`ASK_RULES`]: crate::agent::forced_ask::ASK_RULES
fn forced_ask_settings(settings: &mut Map<String, Value>) {
    let mut permissions = take_object(settings, "permissions");
    let mut ask = take_array(&mut permissions, "ask");
    for rule in crate::agent::forced_ask::ASK_RULES {
        push_new(&mut ask, rule);
    }
    permissions.insert("ask".to_owned(), Value::Array(ask));
    settings.insert("permissions".to_owned(), Value::Object(permissions));
    let mut sandbox = take_object(settings, "sandbox");
    sandbox.insert(
        crate::agent::forced_ask::NO_SANDBOX_AUTO_ALLOW.to_owned(),
        Value::Bool(false),
    );
    settings.insert("sandbox".to_owned(), Value::Object(sandbox));
}

/// Whether an `allowedTools` rule is for a built-in web tool: `WebSearch`, `WebFetch`, or a
/// `WebSearch(…)` or `WebFetch(…)` rule.
fn is_web_rule(rule: &str) -> bool {
    WEB_TOOLS.iter().any(|tool| {
        rule.strip_prefix(tool)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('('))
    })
}

/// The `CODEX_CONFIG` keys the profile and the host add for a Codex agent: the hook's, only
/// those in `codex::PROFILE_KEYS` (any other is refused with a warning), with
/// `developer_instructions` filled; then the host's own `web_search` and
/// `sandbox_workspace_write.writable_roots` (the work and notes folders), which no profile key
/// supplies.
pub(crate) fn codex_keys(
    hook: Map<String, Value>,
    web_search: Switch,
    folders: Folders<'_>,
) -> Result<Map<String, Value>, Error> {
    let mut keys = Map::new();
    for (key, value) in hook {
        if crate::agent::codex::PROFILE_KEYS.contains(&key.as_str()) {
            keys.insert(key, value);
        } else {
            tracing::warn!(
                key,
                "a CODEX_CONFIG key the launch profile may not set is refused"
            );
        }
    }
    if let Some(Value::String(text)) = keys.get_mut("developer_instructions") {
        *text = fill_instructions(text, folders)?;
    }
    let search = if web_search.on { "live" } else { "disabled" };
    keys.insert("web_search".to_owned(), Value::from(search));
    let roots = vec![Value::from(folders.work), Value::from(folders.notes)];
    let mut sandbox = Map::new();
    sandbox.insert("writable_roots".to_owned(), Value::Array(roots));
    keys.insert("sandbox_workspace_write".to_owned(), Value::Object(sandbox));
    Ok(keys)
}

/// `_meta.systemPrompt.append` with its tokens filled, when it is a string.
pub(crate) fn fill_meta(meta: &mut Map<String, Value>, folders: Folders<'_>) -> Result<(), Error> {
    if let Some(Value::String(text)) = meta
        .get_mut("systemPrompt")
        .and_then(Value::as_object_mut)
        .and_then(|prompt| prompt.get_mut("append"))
    {
        *text = fill_instructions(text, folders)?;
    }
    Ok(())
}

/// The instructions with `{work_folder}`, `{matter_notes}`, `{connectors}` and `{browser}`
/// filled, in one pass. An unfillable token is a host bug: `agent.launch_failed`, naming it in
/// the log.
pub(crate) fn fill_instructions(text: &str, folders: Folders<'_>) -> Result<String, Error> {
    placeholders::fill(
        text,
        &IN_INSTRUCTIONS,
        &[
            (WORK_FOLDER, folders.work),
            (MATTER_NOTES, folders.notes),
            (CONNECTORS, folders.connectors),
            (BROWSER, folders.browser),
        ],
    )
    .map_err(|unfilled| {
        tracing::error!(
            token = unfilled.0,
            "an instructions placeholder has no value"
        );
        launch_failed(&unfilled.to_string())
    })
}

/// The web search's header state for an agent of `kind`; none for an agent with no kind.
pub(crate) fn web_search_state(
    kind: Option<AgentKind>,
    switch: Switch,
    login: Login,
) -> Option<ToolState> {
    kind?;
    let availability = if !switch.on {
        ToolAvailability::Unavailable {
            reason: switch.off_reason(),
        }
    } else if kind == Some(AgentKind::Claude) && login == Login::Bedrock {
        ToolAvailability::Unavailable {
            reason: ToolsUnavailableReason::NoSearchOnThisLogin,
        }
    } else {
        ToolAvailability::Available
    };
    Some(ToolState {
        tool: ClauseoTool::WebSearch,
        availability,
    })
}

fn launch_failed(message: &str) -> Error {
    Error::refused(ErrorCode::AgentLaunchFailed, message.to_owned())
}

/// The array at `key`, taken out of `map`; empty when it is missing or not an array.
fn take_array(map: &mut Map<String, Value>, key: &str) -> Vec<Value> {
    match map.remove(key) {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// Pushes `text` unless the array already holds it.
fn push_new(items: &mut Vec<Value>, text: &str) {
    if !items.iter().any(|item| item.as_str() == Some(text)) {
        items.push(Value::from(text));
    }
}

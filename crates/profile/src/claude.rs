//! Claude's part: the options under `_meta.claudeCode.options` and the system prompt append.

use std::path::PathBuf;

use serde_json::{Map, Value};

/// stetdoc's read-only subcommands at v0.9.1, the ones with no output or write flag
/// (`docs/reference/cli.md`: `view` :117-147, `inspect` :229-241, `check` :579-594, `lookup`
/// :612-624, `capabilities` :696-707), allowed without asking.
pub const STETDOC_READ_ONLY: [&str; 5] = ["view", "inspect", "check", "lookup", "capabilities"];

/// agent-browser's subcommands at v0.41.1 allowed without asking (`cli/src/commands.rs` at the
/// tag: `open` :399, `snapshot` :974, `click` :448, `dblclick` :467, `fill` :474, `type` :481,
/// `press` :603, `hover` :524, `focus` :531, `select` :552, `check` :538, `uncheck` :545,
/// `scroll` :660, `scrollintoview` :704, `wait` :713, `get` :1459, `is` :1462, `find` :1465,
/// `screenshot` :872, `pdf` :965, `tab` :1650, `back` :441, `forward` :442, `reload` :443,
/// `close` :1063, `frame` :1717, `dialog` :1730, `download` :590). Every other one (`upload`,
/// `eval`, `state`, `cookies`, `storage`, `network`, `set`, `install`, ...) asks.
pub const AGENT_BROWSER_ALLOWED: [&str; 28] = [
    "open",
    "snapshot",
    "click",
    "dblclick",
    "fill",
    "type",
    "press",
    "hover",
    "focus",
    "select",
    "check",
    "uncheck",
    "scroll",
    "scrollintoview",
    "wait",
    "get",
    "is",
    "find",
    "screenshot",
    "pdf",
    "tab",
    "back",
    "forward",
    "reload",
    "close",
    "frame",
    "dialog",
    "download",
];

/// agent-browser's options and subcommands Claude may never run, whatever the lawyer answers:
/// each attaches the browser to the lawyer's own Chrome or profile, shows a window, swaps
/// Clauseo's config or session, or loads or saves browser state. Claude Code matches `*`
/// anywhere in a rule, spaces included, since 2.1.0, checks deny rules before allow rules, and
/// refuses a denied command without asking (code.claude.com/docs/en/permissions). So
/// `Bash(agent-browser *--profile*)` also catches `--profile=x`, a flag after the subcommand,
/// and the command inside `cd x && …`. A deny list is not a fence: the same options have
/// `AGENT_BROWSER_*` twins that these rules do not see (decision noted in the PR).
pub const AGENT_BROWSER_DENIED: [&str; 9] = [
    "--profile",
    "--auto-connect",
    "--cdp",
    "--executable-path",
    "--headed",
    "--config",
    "--session",
    "--state",
    "--extension",
];

/// agent-browser's subcommands Claude may never run, as the first word after the command
/// (`Bash(agent-browser state*)`): loading or saving browser state, and attaching to a running
/// browser. Not matched anywhere, so a page's text that holds the word (`fill @e1 "state bank"`)
/// is not refused; behind a global flag (`agent-browser --json state load`) the command is in no
/// allow rule, so Claude asks.
pub const AGENT_BROWSER_DENIED_SUBCOMMANDS: [&str; 2] = ["state", "connect"];

/// The keys [`ClaudeOptions::meta_fragment`] may put under `claudeCode.options`. Everything the
/// host forces (`strictMcpConfig`, `settingSources`, `mcpServers`, `env`, `extraArgs`) and the
/// guard's `settings.hooks` and `settings.disableAllHooks` stay out, so the host's merge never
/// meets a profile value for them.
pub const OPTION_KEYS: [&str; 4] = ["allowedTools", "disallowedTools", "plugins", "settings"];

/// The keys the profile sets in `options.settings`.
pub const SETTINGS_KEYS: [&str; 1] = ["disableSkillShellExecution"];

/// What the profile sends a Claude session. The host adds after it: `WebSearch` and the seeded
/// `WebFetch(domain:…)` rules to `allowedTools` when web search is on, `WebSearch` and
/// `WebFetch` to `disallowedTools` when it is off, and `skipWebFetchPreflight` to `settings` on
/// a cloud login.
///
/// The fields are private: [`Self::meta_fragment`] is the only way out, and it emits only
/// [`OPTION_KEYS`] and, in `settings`, only [`SETTINGS_KEYS`].
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeOptions {
    /// The instructions; still holds `{work_folder}` and `{matter_notes}`.
    system_prompt_append: String,
    /// `Bash(stetdoc <sub>:*)` for each of [`STETDOC_READ_ONLY`], then, with the browser,
    /// `Bash(agent-browser <sub>:*)` for each of [`AGENT_BROWSER_ALLOWED`].
    allowed_tools: Vec<String>,
    /// With the browser, the agent-browser denials ([`Self::with_browser`]); otherwise none: the
    /// web tools are the host's, from `tools.web_search`.
    disallowed_tools: Vec<String>,
    /// The local plugin holding the skills.
    plugin_dir: Option<PathBuf>,
    /// The launch's own plugin holding its skill packs (decision record 0040), second.
    pack_plugin: Option<PathBuf>,
    /// `{"disableSkillShellExecution": true}`: no skill runs a shell command on load.
    settings: Map<String, Value>,
}

impl ClaudeOptions {
    /// The options for `instructions` and the plugin at `plugin_dir`.
    pub fn new(instructions: String, plugin_dir: Option<PathBuf>) -> Self {
        let allowed_tools = STETDOC_READ_ONLY
            .iter()
            .map(|sub| format!("Bash(stetdoc {sub}:*)"))
            .collect();
        let mut settings = Map::new();
        settings.insert("disableSkillShellExecution".to_owned(), Value::Bool(true));
        Self {
            system_prompt_append: instructions,
            allowed_tools,
            disallowed_tools: Vec::new(),
            plugin_dir,
            pack_plugin: None,
            settings,
        }
    }

    /// The same options with `Bash(agent-browser <sub>:*)` allowed for each of
    /// [`AGENT_BROWSER_ALLOWED`], and denied: `Bash(agent-browser *<flag>*)` for each of
    /// [`AGENT_BROWSER_DENIED`] and `Bash(agent-browser <sub>*)` for each of
    /// [`AGENT_BROWSER_DENIED_SUBCOMMANDS`]: for a profile whose app put the command on the
    /// agents' `PATH`.
    #[must_use]
    pub fn with_browser(mut self) -> Self {
        self.allowed_tools.extend(
            AGENT_BROWSER_ALLOWED
                .iter()
                .map(|sub| format!("Bash(agent-browser {sub}:*)")),
        );
        self.disallowed_tools.extend(
            AGENT_BROWSER_DENIED
                .iter()
                .map(|flag| format!("Bash(agent-browser *{flag}*)"))
                .chain(
                    AGENT_BROWSER_DENIED_SUBCOMMANDS
                        .iter()
                        .map(|sub| format!("Bash(agent-browser {sub}*)")),
                ),
        );
        self
    }

    /// The same options with the launch's pack plugin at `dir`, as a second `plugins` entry
    /// after the built-in one. Only `ClauseoProfile::session_meta_with_packs` sets it, from a
    /// `PacksWritten`.
    #[must_use]
    pub fn with_pack_plugin(mut self, dir: PathBuf) -> Self {
        self.pack_plugin = Some(dir);
        self
    }

    /// The pack plugin folder.
    pub fn pack_plugin(&self) -> Option<&PathBuf> {
        self.pack_plugin.as_ref()
    }

    /// The same options with `key` set to `value` in `settings`. Only [`SETTINGS_KEYS`] ever
    /// leave in [`Self::meta_fragment`]; any other key set here is dropped there, with a warning.
    #[must_use]
    pub fn with_setting(mut self, key: &str, value: Value) -> Self {
        self.settings.insert(key.to_owned(), value);
        self
    }

    /// The instructions.
    pub fn system_prompt_append(&self) -> &str {
        &self.system_prompt_append
    }

    /// The allow list.
    pub fn allowed_tools(&self) -> &[String] {
        &self.allowed_tools
    }

    /// The deny list: empty from the profile.
    pub fn disallowed_tools(&self) -> &[String] {
        &self.disallowed_tools
    }

    /// The plugin folder.
    pub fn plugin_dir(&self) -> Option<&PathBuf> {
        self.plugin_dir.as_ref()
    }

    /// The `_meta` object: `systemPrompt.append`, and under `claudeCode.options` only
    /// [`OPTION_KEYS`] (`disallowedTools` when it is not empty, `plugins` when there is a
    /// plugin, as one `{"type": "local", "path": …}` each, the built-in plugin first and the
    /// launch's pack plugin second, and `settings` holding only
    /// [`SETTINGS_KEYS`]). So `hooks`, `disableAllHooks` or any other settings key never
    /// reaches the host's merge; each is dropped with a warning. A plugin path that is no UTF-8
    /// is left out with a warning: a lossy copy could name another folder.
    pub fn meta_fragment(&self) -> Map<String, Value> {
        let mut options = Map::new();
        options.insert(
            "allowedTools".to_owned(),
            Value::from(self.allowed_tools.clone()),
        );
        if !self.disallowed_tools.is_empty() {
            options.insert(
                "disallowedTools".to_owned(),
                Value::from(self.disallowed_tools.clone()),
            );
        }
        let mut plugins = Vec::new();
        if let Some(dir) = &self.plugin_dir {
            if let Some(path) = dir.to_str() {
                plugins.push(serde_json::json!({"path": path, "type": "local"}));
            } else {
                tracing::warn!("the Claude plugin folder is not UTF-8: no skills go to Claude");
            }
        }
        if let Some(dir) = &self.pack_plugin {
            if let Some(path) = dir.to_str() {
                plugins.push(serde_json::json!({"path": path, "type": "local"}));
            } else {
                tracing::warn!("the pack plugin folder is not UTF-8: no packs go to Claude");
            }
        }
        if !plugins.is_empty() {
            options.insert("plugins".to_owned(), Value::Array(plugins));
        }
        let mut settings = Map::new();
        for (key, value) in &self.settings {
            if SETTINGS_KEYS.contains(&key.as_str()) {
                settings.insert(key.clone(), value.clone());
            } else {
                tracing::warn!(
                    key,
                    "a Claude settings key the profile may not send is dropped"
                );
            }
        }
        options.insert("settings".to_owned(), Value::Object(settings));
        debug_assert!(
            options
                .keys()
                .all(|key| OPTION_KEYS.contains(&key.as_str()))
        );
        let mut claude = Map::new();
        claude.insert("options".to_owned(), Value::Object(options));
        // Keys inserted in sorted order, so the object reads the same whether or not
        // serde_json's `preserve_order` is on in the build.
        let mut meta = Map::new();
        meta.insert("claudeCode".to_owned(), Value::Object(claude));
        meta.insert(
            "systemPrompt".to_owned(),
            serde_json::json!({"append": self.system_prompt_append}),
        );
        meta
    }
}

//! Claude's part: the options under `_meta.claudeCode.options` and the system prompt append.

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::servers::BROWSER;

/// stetdoc's read-only subcommands at v0.9.1, the ones with no output or write flag
/// (`docs/reference/cli.md`: `view` :117-147, `inspect` :229-241, `check` :579-594, `lookup`
/// :612-624, `capabilities` :696-707), allowed without asking.
pub const STETDOC_READ_ONLY: [&str; 5] = ["view", "inspect", "check", "lookup", "capabilities"];

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
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeOptions {
    /// The instructions; still holds `{work_folder}` and `{matter_notes}`.
    pub system_prompt_append: String,
    /// `mcp__browser`, then `Bash(stetdoc <sub>:*)` for each of [`STETDOC_READ_ONLY`].
    pub allowed_tools: Vec<String>,
    /// None from the profile: the web tools are the host's, from `tools.web_search`.
    pub disallowed_tools: Vec<String>,
    /// The local plugin holding the skills.
    pub plugin_dir: Option<PathBuf>,
    /// `{"disableSkillShellExecution": true}`: no skill runs a shell command on load.
    pub settings: Map<String, Value>,
}

impl ClaudeOptions {
    /// The options for `instructions` and the plugin at `plugin_dir`.
    pub fn new(instructions: String, plugin_dir: Option<PathBuf>) -> Self {
        let mut allowed_tools = vec![format!("mcp__{BROWSER}")];
        allowed_tools.extend(
            STETDOC_READ_ONLY
                .iter()
                .map(|sub| format!("Bash(stetdoc {sub}:*)")),
        );
        let mut settings = Map::new();
        settings.insert("disableSkillShellExecution".to_owned(), Value::Bool(true));
        Self {
            system_prompt_append: instructions,
            allowed_tools,
            disallowed_tools: Vec::new(),
            plugin_dir,
            settings,
        }
    }

    /// The `_meta` object: `systemPrompt.append`, and under `claudeCode.options` only
    /// [`OPTION_KEYS`] (`disallowedTools` when it is not empty, `plugins` when there is a
    /// plugin, as one `{"type": "local", "path": …}`). A plugin path that is no UTF-8 is left
    /// out with a warning: a lossy copy could name another folder.
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
        if let Some(dir) = &self.plugin_dir {
            if let Some(path) = dir.to_str() {
                options.insert(
                    "plugins".to_owned(),
                    serde_json::json!([{"path": path, "type": "local"}]),
                );
            } else {
                tracing::warn!(
                    path = %dir.display(),
                    "the Claude plugin folder is not UTF-8: no skills go to Claude"
                );
            }
        }
        options.insert("settings".to_owned(), Value::Object(self.settings.clone()));
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

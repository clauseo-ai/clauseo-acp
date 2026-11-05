//! The settings schema: [`Settings`], its sections and their value types.
//!
//! `settings.json` and every `policy.json` have this shape. M2 adds sections; it never moves a
//! key. [`json_schema`] gives the JSON Schema, for tests and docs only: `Effective::load` never
//! builds it.

#[cfg(test)]
mod tests;

use crate::ui::UiPatch;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The top-level keys of `settings.json`, each an object.
pub(crate) const SECTIONS: [&str; 10] = [
    "matters", "agents", "mcp", "ui", "tools", "privacy", "update", "plans", "install", "skills",
];

/// Every setting, as `Effective::settings` gives it: the defaults, then `settings.json`, then
/// the policy layer.
// Serialize only, for `settings_get`: the one way to read settings is `Effective::load`, which
// checks agent commands, warns and applies policy. (A `///` line here would move the schema.)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct Settings {
    /// Where matters live.
    pub matters: MattersSettings,
    /// The agents Clauseo can launch, by id. Empty by default.
    pub agents: BTreeMap<String, AgentLaunch>,
    /// The MCP servers a lawyer can turn on for their agents' sessions.
    pub mcp: McpSettings,
    /// How the app looks. The webview changes these only through a `UiPatch`.
    pub ui: UiSettings,
    /// Clauseo's own tools an agent may use. Read at start-up; a change applies after a restart.
    pub tools: ToolsSettings,
    /// What Clauseo may count and send about its own use (decision record 0036).
    pub privacy: PrivacySettings,
    /// How Clauseo updates itself (task R23).
    pub update: UpdateSettings,
    /// Each agent's firm plan, by agent id (decision record 0048). Empty by default.
    pub plans: BTreeMap<String, Plan>,
    /// Where agent installs come from in a firm (decision record 0048).
    pub install: InstallSettings,
    /// The extra skill packs Clauseo may load (decision record 0040).
    pub skills: SkillsSettings,
}

/// The `matters` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct MattersSettings {
    /// The folder new matters go in, as an absolute path. When absent: `Documents/Clauseo`, or,
    /// with `CLAUSEO_DATA_DIR` set, `<data dir>-matters` beside the data folder.
    pub default_location: Option<PathBuf>,
}

/// How to start one agent. `process` and `host` launch it; this crate only reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AgentLaunch {
    /// The program, as an absolute path.
    pub command: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables added to its environment. Never a secret: keys go in the OS keychain.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// One agent's firm plan (`plans.<id>`): a gateway or a cloud provider instead of the plan
/// login. A security key: a policy source that cannot be read leaves no plan at all, so no
/// endpoint the lawyer chose stands in for the firm's (decision record 0048).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    /// The firm's gateway: an `https` URL with a host, and no user name, password, query or
    /// fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Whose service the plan uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<PlanProvider>,
    /// Extra variables for the agent. A name holding `KEY`, `TOKEN`, `SECRET` or `PASSWORD`,
    /// in any case, is refused: secrets stay with IT's own tools and the OS keychain.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// Whose service a firm plan uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanProvider {
    /// Anthropic's API, or a gateway that speaks it.
    Anthropic,
    /// Amazon Bedrock.
    Bedrock,
    /// Google Vertex AI.
    Vertex,
}

/// The `install` section: where agent installs come from in a firm. Both keys are security
/// keys: a policy source that cannot be read leaves both unset, so installs use the pinned
/// public hosts with their integrity checks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct InstallSettings {
    /// An offline bundle folder, as an absolute path. Unset by default.
    pub bundle: Option<PathBuf>,
    /// A firm npm mirror: an `https` URL with a host, and no user name, password, query or
    /// fragment. Unset by default.
    pub registry: Option<String>,
}

/// The `skills` section (decision record 0040). `extra`, `curated` and `allow_scripts` are
/// security keys: a policy source that cannot be read, or a policy value that does not type,
/// locks `extra` `off`, `allow_scripts` `false` and `curated` empty. `curated` is set by
/// policy only: a `settings.json` value is ignored with a warning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct SkillsSettings {
    /// Which packs beyond Clauseo's own may load. `off` by default.
    pub extra: SkillsExtra,
    /// The pack ids `curated` admits. Set by policy only; empty by default.
    pub curated: Vec<String>,
    /// Whether a pack may carry scripts (copied, never run). Off by default.
    pub allow_scripts: bool,
    /// The packs turned on, by id, each with the hash it had then (`sha256:<64 lowercase hex>`):
    /// a pack whose files no longer hash to it is not loaded. Empty by default.
    pub enabled: BTreeMap<String, String>,
}

/// Which packs beyond Clauseo's own may load.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkillsExtra {
    /// None.
    #[default]
    Off,
    /// Only those whose id is in `skills.curated`.
    Curated,
    /// Any pack the lawyer turns on.
    Any,
}

/// The `mcp` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct McpSettings {
    /// The MCP servers by name: 1 to 64 letters, digits, `_` or `-`. Each is off until it is
    /// turned on. A policy value replaces the whole list. Empty by default.
    pub servers: BTreeMap<String, McpServerLaunch>,
}

/// How to start one MCP server. `host` passes the enabled ones to the agent in
/// `session/new`; this crate only reads them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct McpServerLaunch {
    /// The program, as an absolute path.
    pub command: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Variables added to its environment. Stored in plain text in `settings.json` and sent to
    /// the agent in `session/new`: do not put a token here. No keychain store for MCP keys
    /// exists yet.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Whether the agent gets it. Off unless set.
    #[serde(default)]
    pub enabled: bool,
}

/// The `tools` section: Clauseo's own tools. Every key is a security key: a policy source that
/// cannot be read, or a policy value that does not type, locks each `false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct ToolsSettings {
    /// Whether the agent may search the web and read web pages. On by default.
    pub web_search: bool,
    /// Whether the agent gets Clauseo's browser. On by default on macOS; off elsewhere, and on
    /// Windows until G17 passes.
    pub browser: bool,
    /// Whether data connectors (R24's paid case-law connector) may run. On by default, but
    /// nothing runs until the lawyer adds a connector. A firm turns every connector off here.
    pub connectors: bool,
}

impl Default for ToolsSettings {
    fn default() -> Self {
        ToolsSettings {
            web_search: true,
            browser: cfg!(target_os = "macos"),
            connectors: true,
        }
    }
}

/// The `privacy` section: the anonymous usage counts and crash reports (decision record 0036).
/// Both keys are privacy keys: a policy can only turn them off. A policy `true` is ignored with a
/// warning and locks nothing; a policy `false`, a policy value that does not type, and a policy
/// source that cannot be read each lock the key `false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct PrivacySettings {
    /// Whether Clauseo keeps and sends its anonymous usage counts. On by default; nothing is
    /// kept or sent before the lawyer's first act.
    pub counts: bool,
    /// Whether Clauseo sends crash reports. Off by default.
    pub crash_reports: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        PrivacySettings {
            counts: true,
            crash_reports: false,
        }
    }
}

/// The `update` section. `update.mode` is a security key: a policy may set any of its values,
/// which locks it, and a policy source that cannot be read, or a policy value that does not
/// type, locks it `none`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct UpdateSettings {
    /// When Clauseo installs an update. `auto` by default.
    pub mode: UpdateMode,
}

/// When Clauseo installs an update. The app writes only `auto` and `notify`; `none` comes from
/// policy, or a hand-edited `settings.json`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMode {
    /// Download in the background, and install at the next start or when the lawyer says.
    #[default]
    Auto,
    /// Download, and install only when the lawyer says.
    Notify,
    /// Never check: the firm updates Clauseo itself.
    None,
}

/// The `ui` section. Design may change these values before M4 without a decision record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct UiSettings {
    /// Light, dark, or as the OS is set.
    pub theme: Theme,
    /// The text size, in points.
    pub font_size: FontSize,
    /// How much space the app leaves round things.
    pub density: Density,
    /// The app's language.
    pub language: Language,
    /// Whether the app drops its heavier effects, or decides from the machine.
    pub lite: Lite,
    /// Whether the app animates, or follows the OS's reduce-motion setting.
    pub motion: Motion,
}

/// The colour theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// As the OS is set.
    #[default]
    System,
    /// Light.
    Light,
    /// Dark.
    Dark,
}

/// How much space the app leaves round things.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    /// More space.
    #[default]
    Comfortable,
    /// Less space.
    Compact,
}

/// Whether the app runs light: fewer effects, for slow machines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Lite {
    /// The app decides from the machine.
    #[default]
    Automatic,
    /// Always light.
    On,
    /// Never light.
    Off,
}

/// Whether the app animates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    /// As the OS's reduce-motion setting says.
    #[default]
    System,
    /// Little or no animation.
    Reduce,
    /// Full animation.
    Full,
}

/// A text size from [`FontSize::MIN`] to [`FontSize::MAX`] points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct FontSize(u8);

/// The app's language: one the app has words for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// English.
    #[default]
    En,
    /// Hindi.
    Hi,
}

impl Settings {
    /// Sets each `ui` field the patch holds. Only [`crate::Effective::apply_ui_patch`] calls it,
    /// after the write has succeeded, so the file and memory never disagree.
    pub(crate) fn apply_ui_patch(&mut self, patch: &UiPatch) {
        // Every field is named here, so a new field does not compile until it is applied.
        let UiPatch {
            theme,
            font_size,
            density,
            language,
            lite,
            motion,
        } = patch;
        if let Some(theme) = theme {
            self.ui.theme = *theme;
        }
        if let Some(font_size) = font_size {
            self.ui.font_size = *font_size;
        }
        if let Some(density) = density {
            self.ui.density = *density;
        }
        if let Some(language) = language {
            self.ui.language = *language;
        }
        if let Some(lite) = lite {
            self.ui.lite = *lite;
        }
        if let Some(motion) = motion {
            self.ui.motion = *motion;
        }
    }
}

impl FontSize {
    /// The smallest size.
    pub const MIN: u8 = 14;
    /// The largest size.
    pub const MAX: u8 = 20;
    /// The size when none is set.
    pub const DEFAULT: u8 = 16;

    /// The size in points.
    pub fn get(self) -> u8 {
        self.0
    }
}

impl Default for FontSize {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

impl TryFrom<u8> for FontSize {
    type Error = String;

    fn try_from(size: u8) -> Result<Self, String> {
        if (Self::MIN..=Self::MAX).contains(&size) {
            Ok(Self(size))
        } else {
            Err(format!(
                "a font size is from {} to {}, not {size}",
                Self::MIN,
                Self::MAX
            ))
        }
    }
}

impl From<FontSize> for u8 {
    fn from(size: FontSize) -> u8 {
        size.0
    }
}

impl JsonSchema for FontSize {
    fn schema_name() -> Cow<'static, str> {
        "FontSize".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A text size, in points.",
            "type": "integer",
            "minimum": Self::MIN,
            "maximum": Self::MAX,
            "default": Self::DEFAULT,
        })
    }
}

/// The JSON Schema of `settings.json`. For tests and docs; never on the start-up path.
pub fn json_schema() -> serde_json::Value {
    schemars::schema_for!(Settings).to_value()
}

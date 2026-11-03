//! The display settings as the window reads and changes them: [`SettingsView`] answers
//! `HostQuery::GetSettings` and rides `HostEvent::SettingsChanged`; a [`UiPatch`] is what
//! `HostCommand::SetUiSettings` carries. The tool switches are a [`ToolsSettingsView`] inside
//! it, changed by a [`ToolsPatch`] in `HostCommand::SetToolSettings`.
//!
//! These mirror the `ui` section of `clauseo-settings`, which this crate cannot depend on. That
//! crate holds the rules (the font size range, which keys policy locks); the host turns these
//! into its types and back. A test in `crates/host` fails when a key or a value is in one and
//! not the other.

use serde::{Deserialize, Serialize};

/// The colour theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Theme {
    /// As the OS is set.
    #[default]
    System,
    /// Light.
    Light,
    /// Dark.
    Dark,
}

impl Theme {
    /// Every value, in declaration order.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];
}

/// The app's language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Language {
    /// English.
    #[default]
    En,
    /// Hindi.
    Hi,
}

impl Language {
    /// Every value, in declaration order.
    pub const ALL: [Self; 2] = [Self::En, Self::Hi];
}

/// Whether the app runs light: fewer effects, for slow machines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Lite {
    /// The app decides from the machine.
    #[default]
    Automatic,
    /// Always light.
    On,
    /// Never light.
    Off,
}

impl Lite {
    /// Every value, in declaration order.
    pub const ALL: [Self; 3] = [Self::Automatic, Self::On, Self::Off];
}

/// Whether the app animates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Motion {
    /// As the OS's reduce-motion setting says.
    #[default]
    System,
    /// Little or no animation.
    Reduce,
    /// Full animation.
    Full,
}

impl Motion {
    /// Every value, in declaration order.
    pub const ALL: [Self; 3] = [Self::System, Self::Reduce, Self::Full];
}

/// The display settings in force: the user's, with policy's values over them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UiSettings {
    /// The colour theme.
    pub theme: Theme,
    /// The text size in points, 14 to 20 (the settings crate holds the range).
    pub font_size: u8,
    /// The app's language.
    pub language: Language,
    /// Whether the app runs light.
    pub lite: Lite,
    /// Whether the app animates.
    pub motion: Motion,
}

/// What `GetSettings` answers and `SettingsChanged` carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SettingsView {
    /// The display settings.
    pub ui: UiSettings,
    /// The tool switches in force. A session reads them at its start: a change reaches the
    /// next session started, and a session already running keeps its tools.
    pub tools: ToolsSettingsView,
    /// Every dotted key the firm's policy sets, such as `ui.theme`, sorted. The lawyer cannot
    /// change these: a `SetUiSettings` or `SetToolSettings` naming one is refused with
    /// `settings.locked`. It can also name keys outside `ui` and `tools`
    /// (`matters.default_location`, `agents.<id>`).
    pub locked: Vec<String>,
}

/// The tool switches in force: the lawyer's, with policy's values over them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ToolsSettingsView {
    /// Whether the agent may search the web and read web pages (`tools.web_search`).
    pub web_search: bool,
    /// Whether the agent gets Clauseo's browser (`tools.browser`).
    pub browser: bool,
}

/// A change to the tool switches: each field set is changed, the rest stay. A patch that names
/// a locked key is refused whole with `settings.locked`, and changes nothing. Unknown fields are
/// refused, as in every command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ToolsPatch {
    /// The new web search setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search: Option<bool>,
    /// The new browser setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<bool>,
}

/// A change to the display settings: each field set is changed, the rest stay. A patch that
/// names a locked key is refused whole with `settings.locked`; a font size out of range with
/// `command.invalid`. Unknown fields are refused, as in every command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UiPatch {
    /// The new theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    /// The new text size in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u8>,
    /// The new language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    /// The new lite setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lite: Option<Lite>,
    /// The new motion setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<Motion>,
}

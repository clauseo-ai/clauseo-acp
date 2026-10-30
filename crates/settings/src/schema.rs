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
pub(crate) const SECTIONS: [&str; 3] = ["matters", "agents", "ui"];

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
    /// How the app looks. The webview changes these only through a `UiPatch`.
    pub ui: UiSettings,
}

/// The `matters` section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(default)]
pub struct MattersSettings {
    /// The folder new matters go in, as an absolute path. When absent, `Documents/Clauseo`.
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

/// A text size from [`FontSize::MIN`] to [`FontSize::MAX`] points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct FontSize(u8);

/// A BCP 47 language tag, such as `en` or `en-GB`: a first part of 2 to 8 letters, then parts
/// of 1 to 8 letters or digits, joined by `-`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Language(String);

impl Settings {
    /// Sets each `ui` field the patch holds. This is the one way the webview changes settings.
    pub fn apply_ui_patch(&mut self, patch: &UiPatch) {
        if let Some(theme) = patch.theme {
            self.ui.theme = theme;
        }
        if let Some(font_size) = patch.font_size {
            self.ui.font_size = font_size;
        }
        if let Some(density) = patch.density {
            self.ui.density = density;
        }
        if let Some(language) = &patch.language {
            self.ui.language = language.clone();
        }
    }
}

impl FontSize {
    /// The smallest size.
    pub const MIN: u8 = 12;
    /// The largest size.
    pub const MAX: u8 = 20;
    /// The size when none is set.
    pub const DEFAULT: u8 = 14;

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

impl Language {
    /// The language when none is set.
    pub const DEFAULT: &str = "en";

    /// The tag.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Language {
    fn default() -> Self {
        Self(Self::DEFAULT.to_owned())
    }
}

impl TryFrom<String> for Language {
    type Error = String;

    fn try_from(tag: String) -> Result<Self, String> {
        let mut parts = tag.split('-');
        let first_ok = parts.next().is_some_and(|first| {
            part_ok(first, 2) && first.bytes().all(|b| b.is_ascii_alphabetic())
        });
        if first_ok && parts.all(|part| part_ok(part, 1)) {
            Ok(Self(tag))
        } else {
            Err(format!("{tag:?} is not a language tag such as en or en-GB"))
        }
    }
}

/// Whether one part of a language tag has `min` to 8 ASCII letters or digits.
fn part_ok(part: &str, min: usize) -> bool {
    (min..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl From<Language> for String {
    fn from(language: Language) -> String {
        language.0
    }
}

impl JsonSchema for Language {
    fn schema_name() -> Cow<'static, str> {
        "Language".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A BCP 47 language tag, such as en or en-GB.",
            "type": "string",
            "default": Self::DEFAULT,
        })
    }
}

/// The JSON Schema of `settings.json`. For tests and docs; never on the start-up path.
pub fn json_schema() -> serde_json::Value {
    schemars::schema_for!(Settings).to_value()
}

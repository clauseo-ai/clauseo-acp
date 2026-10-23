//! The webview's settings: [`UiPatch`] carries only the four `ui` fields.

#[cfg(test)]
mod tests;

use crate::schema::{Density, FontSize, Language, Theme};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A change to the `ui` section, as the webview sends it. It refuses every other key, so the
/// webview cannot reach agents, folders or policy. Apply it with
/// [`crate::Settings::apply_ui_patch`] and save it with [`crate::write_user_settings`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiPatch {
    /// The new theme, if it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    /// The new text size, if it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<FontSize>,
    /// The new density, if it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density: Option<Density>,
    /// The new language, if it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
}

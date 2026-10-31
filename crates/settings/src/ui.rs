//! The webview's settings: [`UiPatch`] carries only the four `ui` fields.

#[cfg(test)]
mod tests;

use crate::schema::{Density, FontSize, Language, Theme};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A change to the `ui` section, as the webview sends it. It refuses every other key, so the
/// webview cannot reach agents, folders or policy. Apply it with
/// [`crate::Effective::apply_ui_patch`], which refuses a locked key and saves the rest.
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

impl UiPatch {
    /// The dotted keys this patch sets, in field order (theme, font size, density, language).
    /// A key the patch leaves out is not in the list, so an empty list means an empty patch.
    pub(crate) fn keys(&self) -> Vec<&'static str> {
        // Every field is named here, so a new field does not compile until it is listed.
        let UiPatch {
            theme,
            font_size,
            density,
            language,
        } = self;
        [
            ("ui.theme", theme.is_some()),
            ("ui.font_size", font_size.is_some()),
            ("ui.density", density.is_some()),
            ("ui.language", language.is_some()),
        ]
        .into_iter()
        .filter_map(|(key, set)| set.then_some(key))
        .collect()
    }
}

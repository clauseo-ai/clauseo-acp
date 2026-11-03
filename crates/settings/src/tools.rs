//! The webview's tool switches: [`ToolsPatch`] carries only the `tools` fields. It is the
//! `tools` counterpart of [`crate::UiPatch`]; the two stay apart so each section's runtime change
//! is one block.

use crate::schema::ToolsSettings;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A change to the `tools` section, as the webview sends it. It refuses every other key. Apply
/// it with [`crate::Effective::apply_tools_patch`], which refuses a locked key and saves the rest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolsPatch {
    /// The new `tools.web_search`, if it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search: Option<bool>,
    /// The new `tools.browser`, if it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<bool>,
}

impl ToolsPatch {
    /// The dotted keys this patch sets, in field order (web search, browser). A key the patch
    /// leaves out is not in the list, so an empty list means an empty patch.
    pub(crate) fn keys(&self) -> Vec<&'static str> {
        // Every field is named here, so a new field does not compile until it is listed.
        let ToolsPatch {
            web_search,
            browser,
        } = self;
        [
            ("tools.web_search", web_search.is_some()),
            ("tools.browser", browser.is_some()),
        ]
        .into_iter()
        .filter_map(|(key, set)| set.then_some(key))
        .collect()
    }
}

impl ToolsSettings {
    /// Sets each field the patch holds. Only [`crate::Effective::apply_tools_patch`] calls it,
    /// after the write has succeeded, so the file and memory never disagree.
    pub(crate) fn apply_patch(&mut self, patch: &ToolsPatch) {
        // Every field is named here, so a new field does not compile until it is applied.
        let ToolsPatch {
            web_search,
            browser,
        } = patch;
        if let Some(web_search) = web_search {
            self.web_search = *web_search;
        }
        if let Some(browser) = browser {
            self.browser = *browser;
        }
    }
}

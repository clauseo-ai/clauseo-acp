//! The lawyer's privacy switches: [`PrivacyPatch`] carries only the `privacy` fields. It is the
//! `privacy` counterpart of [`crate::ToolsPatch`]; each section's runtime change stays one block.

use crate::schema::PrivacySettings;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A change to the `privacy` section. It refuses every other key. Apply it with
/// [`crate::Effective::apply_privacy_patch`], which refuses a locked key and saves the rest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrivacyPatch {
    /// The new `privacy.counts`, if it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<bool>,
    /// The new `privacy.crash_reports`, if it changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crash_reports: Option<bool>,
}

impl PrivacyPatch {
    /// The dotted keys this patch sets, in field order (counts, crash reports). A key the patch
    /// leaves out is not in the list, so an empty list means an empty patch.
    pub(crate) fn keys(&self) -> Vec<&'static str> {
        // Every field is named here, so a new field does not compile until it is listed.
        let PrivacyPatch {
            counts,
            crash_reports,
        } = self;
        [
            ("privacy.counts", counts.is_some()),
            ("privacy.crash_reports", crash_reports.is_some()),
        ]
        .into_iter()
        .filter_map(|(key, set)| set.then_some(key))
        .collect()
    }
}

impl PrivacySettings {
    /// Sets each field the patch holds. Only [`crate::Effective::apply_privacy_patch`] calls it,
    /// after the write has succeeded, so the file and memory never disagree.
    pub(crate) fn apply_patch(&mut self, patch: &PrivacyPatch) {
        // Every field is named here, so a new field does not compile until it is applied.
        let PrivacyPatch {
            counts,
            crash_reports,
        } = patch;
        if let Some(counts) = counts {
            self.counts = *counts;
        }
        if let Some(crash_reports) = crash_reports {
            self.crash_reports = *crash_reports;
        }
    }
}

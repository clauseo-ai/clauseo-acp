//! The lawyer's update mode: [`UpdateChoice`] is the `update.mode` the app may write, `auto` or
//! `notify`. `none` is never written from the app: it comes from policy, or from a hand-edited
//! `settings.json` (task R23).

use crate::schema::UpdateMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// An `update.mode` the lawyer can pick. Apply it with [`crate::Effective::apply_update_mode`],
/// which refuses it while policy locks the key. It has no `none`, so the app cannot turn
/// updates off for a lawyer: only the firm's policy (or the lawyer's own hand edit) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChoice {
    /// Download in the background and install at the next start.
    Auto,
    /// Download, and install only when the lawyer says so.
    Notify,
}

impl From<UpdateChoice> for UpdateMode {
    fn from(choice: UpdateChoice) -> UpdateMode {
        match choice {
            UpdateChoice::Auto => UpdateMode::Auto,
            UpdateChoice::Notify => UpdateMode::Notify,
        }
    }
}

/// What [`crate::user::write_update_mode`] saves into the `update` section.
#[derive(Serialize)]
pub(crate) struct UpdateFields {
    pub mode: UpdateChoice,
}

//! The entries of a helper (a subagent the lead agent launched): its card and its words. Its
//! steps are `ToolCall` entries whose `helper` names it. Every entry sorts by `order`; a
//! screen groups a helper's steps and words by `helper`, in `order`.

use serde::{Deserialize, Serialize};

use crate::{
    CompactString, EventId, HelperId, HelperOutcome, Seq, TextOrBlob, Timestamp, ToolCallId, TurnId,
};

/// A helper: `HelperStarted` with its `HelperEnded` applied. Entry id `helper:<helper>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HelperEntry {
    /// The `seq` of `HelperStarted`.
    pub order: Seq,
    /// The helper.
    pub helper: HelperId,
    /// The turn it started in.
    pub turn: TurnId,
    /// The helper that launched it; absent when the lead did.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent: Option<HelperId>,
    /// The step that launched it; the card stands in that step's place.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub call: Option<ToolCallId>,
    /// Its name.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub name: CompactString,
    /// The agent's short label for the task.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub label: Option<CompactString>,
    /// What it was asked; absent when the agent did not say.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub task: Option<TextOrBlob>,
    /// The model it runs on, when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub model: Option<CompactString>,
    /// How it ended; absent while it runs.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<HelperOutcome>,
    /// What it reported back, once ended.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<TextOrBlob>,
    /// When it started: the `ts` of its `HelperStarted` envelope.
    pub started_at: Timestamp,
    /// When it ended: the `ts` of its `HelperEnded` envelope; absent while it runs.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub finished_at: Option<Timestamp>,
}

/// A run of a helper's own words: one `HelperText`. Entry id `helper_text:<event>`. A screen
/// joins the runs of one helper that no step of that helper separates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HelperTextEntry {
    /// The `seq` of its `HelperText`.
    pub order: Seq,
    /// The id of its `HelperText` event.
    pub event: EventId,
    /// Its turn.
    pub turn: TurnId,
    /// The helper whose words these are.
    pub helper: HelperId,
    /// The words.
    pub text: String,
}

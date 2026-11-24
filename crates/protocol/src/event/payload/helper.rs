//! Payloads about a helper (a subagent the lead agent launched): its start, its own words and
//! its end. Its steps are `ToolCallStarted` events with `helper` set.

use serde::{Deserialize, Serialize};

use crate::{CompactString, HelperId, HelperOutcome, TextOrBlob, ToolCallId, TurnId};

/// The lead agent (or another helper) launched a helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HelperStarted {
    /// The turn it started in.
    pub turn: TurnId,
    /// Clauseo's id for it.
    pub helper: HelperId,
    /// The agent's id for it, unique only within one agent session (Claude: the `toolCallId`
    /// of the lead's `Agent` call that launched it).
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub acp_id: CompactString,
    /// The helper that launched it; absent when the lead did.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent: Option<HelperId>,
    /// The step that launched it (Claude: the lead's `Agent` call); its card stands in that
    /// step's place.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub call: Option<ToolCallId>,
    /// Its name: the helper's type (`paper-reader`), else the name the agent gave it.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub name: CompactString,
    /// A short label the agent gave the task (Claude: the `Agent` call's `description`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub label: Option<CompactString>,
    /// What it was asked (Claude: the `Agent` call's `prompt`); a blob when large, as a
    /// step's `raw_input` is. Absent when the agent does not say (Codex hides it).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub task: Option<TextOrBlob>,
    /// The model it runs on, when known: the call's own, else the one Clauseo set for that
    /// helper.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub model: Option<CompactString>,
}

/// A run of a helper's own words, after its earlier words and steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HelperText {
    /// The turn whose log holds it.
    pub turn: TurnId,
    /// The helper.
    pub helper: HelperId,
    /// The words: at most [`INLINE_LIMIT_BYTES`](crate::INLINE_LIMIT_BYTES) after JSON
    /// escaping, as the host cuts them.
    pub text: String,
}

/// A helper ended; the host logs it at most once per helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HelperEnded {
    /// The helper.
    pub helper: HelperId,
    /// How it ended.
    pub outcome: HelperOutcome,
    /// What it reported back (Claude: the text of the `Agent` call's result), or the agent's
    /// note when it cut the helper at its turn limit; a blob when large.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<TextOrBlob>,
}

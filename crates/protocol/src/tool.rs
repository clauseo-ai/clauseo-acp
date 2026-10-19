//! Tool calls: what an agent's tool does, where it works, and how it is reported.
//!
//! [`ToolKind`] and [`ToolCallStatus`] copy ACP v1's enums (schema 1.11.0) as Clauseo's own
//! types. Clauseo mints the [`ToolCallId`]; ACP's `toolCallId` is kept as `acp_id`, a foreign
//! attribute, because ACP ids are unique only within one agent session.

use serde::{Deserialize, Serialize};

use crate::open_enum::open_unit_enum;
use crate::{CompactString, MatterPath, TextOrBlob, ToolCallContent, ToolCallId, TurnId};

open_unit_enum! {
    /// What a tool call does, as ACP v1 names it. The UI picks an icon by kind.
    pub enum ToolKind {
        /// Reads files or data.
        Read,
        /// Edits files.
        Edit,
        /// Deletes files.
        Delete,
        /// Moves or renames files.
        Move,
        /// Searches.
        Search,
        /// Runs a command.
        Execute,
        /// Thinks or plans, with no side effect.
        Think,
        /// Fetches from the network.
        Fetch,
        /// Switches the session's mode.
        SwitchMode,
        /// Anything else.
        Other,
    }
}

open_unit_enum! {
    /// Where a tool call stands: ACP v1's statuses plus Clauseo's `Cancelled`.
    pub enum ToolCallStatus {
        /// Not started; its input may still be streaming, or it waits for a permission.
        Pending,
        /// Running.
        InProgress,
        /// Finished.
        Completed,
        /// Failed.
        Failed,
        /// Still running when its turn was cancelled; the host marks it so.
        Cancelled,
    }
}

/// A place a tool call touches. Paths are matter-relative; anything outside the matter keeps
/// only display text, so the UI flags it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ToolLocation {
    /// A file inside the matter.
    Inside {
        /// The file, relative to the matter folder.
        path: MatterPath,
        /// A 1-based line in it, when the agent gave one.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        line: Option<u32>,
    },
    /// A place outside the matter. The UI shows it flagged and never opens it.
    Outside {
        /// The path as the agent gave it, for display only.
        display: String,
    },
}

/// The payload of `DomainEvent::ToolCallStarted`: a tool call the agent reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ToolCallStarted {
    /// The turn it runs in.
    pub turn: TurnId,
    /// Clauseo's id for it.
    pub tool_call: ToolCallId,
    /// ACP's `toolCallId`, unique only within one agent session.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub acp_id: CompactString,
    /// A title to show.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub title: CompactString,
    /// What it does.
    pub kind: ToolKind,
    /// The programmatic tool name; rendering is looked up by it, the generic card the fallback.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub tool_name: Option<CompactString>,
    /// Where it stands.
    pub status: ToolCallStatus,
    /// The places it touches.
    pub locations: Vec<ToolLocation>,
    /// Its input, as compact JSON text.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_input: Option<TextOrBlob>,
    /// Its output, as compact JSON text.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_output: Option<TextOrBlob>,
    /// What it shows.
    pub content: Vec<ToolCallContent>,
}

/// A change to a tool call: [`ToolCallStarted`]'s fields after `acp_id`, each `None` when it did
/// not change. `content` and `locations`, when present, replace the old list wholesale; nothing
/// is merged by position.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ToolCallPatch {
    /// The new title.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub title: Option<CompactString>,
    /// The new kind.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub kind: Option<ToolKind>,
    /// The new tool name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub tool_name: Option<CompactString>,
    /// The new status.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub status: Option<ToolCallStatus>,
    /// The new places, replacing the old list.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub locations: Option<Vec<ToolLocation>>,
    /// The new input.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_input: Option<TextOrBlob>,
    /// The new output.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub raw_output: Option<TextOrBlob>,
    /// The new content, replacing the old list.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content: Option<Vec<ToolCallContent>>,
}

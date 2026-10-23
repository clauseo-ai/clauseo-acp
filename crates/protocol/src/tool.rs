//! Tool calls: what an agent's tool does, where it works, and how it is reported.
//!
//! [`ToolKind`] and [`ToolCallStatus`] copy ACP v1's enums (schema 1.11.0) as Clauseo's own
//! types. Clauseo mints the [`ToolCallId`]; ACP's `toolCallId` is kept as `acp_id`, a foreign
//! attribute, because ACP ids are unique only within one agent session.

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};
use crate::{CompactString, MatterPath, TextOrBlob, ToolCallContent};

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
    test mod tool_kind;
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
    test mod tool_call_status;
}

open_data_enum! {
    /// A place a tool call touches. Paths are matter-relative; anything outside the matter
    /// keeps only display text, so the UI flags it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ToolLocation {
        /// A file inside the matter.
        Inside = "inside" {
            /// The file, relative to the matter folder.
            path: MatterPath,
            /// A 1-based line in it, when the agent gave one.
            #[serde(skip_serializing_if = "Option::is_none", default)]
            line: Option<u32>,
        },
        /// A place outside the matter. The UI shows it flagged and never opens it.
        Outside = "outside" {
            /// The path as the agent gave it, for display only.
            display: String,
        },
        _ =>
        /// A place from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod tool_location with [
        ToolLocation::Inside {
            path: MatterPath::new("a.docx").unwrap(),
            line: Some(3),
        },
        ToolLocation::Outside {
            display: "~/notes.txt".to_owned(),
        },
    ];
}

/// A change to a tool call: [`ToolCallStarted`](crate::ToolCallStarted)'s fields after
/// `acp_id`, each `None` when it did not change. `content` and `locations`, when present,
/// replace the old list wholesale; nothing is merged by position.
///
/// Since `None` means unchanged, no patch takes a field back to absent: once a call has a
/// `tool_name`, `raw_input` or `raw_output`, it keeps one. A new tool-call field touches three
/// structs, `ToolCallStarted`, this patch and the view's `ToolCallEntry`, plus the reducer's
/// merge.
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

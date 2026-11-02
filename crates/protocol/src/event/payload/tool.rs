//! Payloads about a tool call, a web search or page, and what the agent asks the lawyer
//! (permissions, elicitations), logged in the thread's stream.

use serde::{Deserialize, Serialize};

use crate::{
    AgentId, CompactString, Deliverable, ElicitationId, ElicitationOutcome, ElicitationRequest,
    ExpiryReason, MatterPath, PageVia, PermissionChoice, PermissionOption, PermissionRequestId,
    Principal, TextOrBlob, ThreadId, Timestamp, ToolCallContent, ToolCallId, ToolCallPatch,
    ToolCallStatus, ToolKind, ToolLocation, TurnId,
};

/// A tool call the agent reported.
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

/// A tool call changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ToolCallUpdated {
    /// The tool call.
    pub tool_call: ToolCallId,
    /// What changed.
    pub patch: ToolCallPatch,
}

/// The agent asked for permission; a durable record any client may answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PermissionRequested {
    /// The request.
    pub request: PermissionRequestId,
    /// The tool call it is about, when there is one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call: Option<ToolCallId>,
    /// The options offered.
    pub options: Vec<PermissionOption>,
    /// Whether an answer can reach the agent.
    pub deliverable: Deliverable,
}

/// A permission request was answered; the first valid answer wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PermissionResolved {
    /// The request.
    pub request: PermissionRequestId,
    /// The answer.
    pub choice: PermissionChoice,
    /// Who answered.
    pub by: Principal,
    /// A program holds this file open: an Office owner file beside it, or on Windows a sharing
    /// violation; the host refused the request without asking. The choice is then the
    /// request's reject-once option, or `Cancelled` when it offered none.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub held_open: Option<MatterPath>,
}

/// A permission request expired unanswered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PermissionExpired {
    /// The request.
    pub request: PermissionRequestId,
    /// Why.
    pub reason: ExpiryReason,
}

/// The agent asked the lawyer for input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ElicitationRequested {
    /// The elicitation.
    pub elicitation: ElicitationId,
    /// What it asks for.
    pub request: ElicitationRequest,
}

/// An elicitation was answered. The values are not logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ElicitationResolved {
    /// The elicitation.
    pub elicitation: ElicitationId,
    /// How it ended.
    pub outcome: ElicitationOutcome,
    /// Who answered.
    pub by: Principal,
}

/// An elicitation expired unanswered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ElicitationExpired {
    /// The elicitation.
    pub elicitation: ElicitationId,
    /// Why.
    pub reason: ExpiryReason,
}

/// A web search the agent ran, with the results it got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WebSearchRecorded {
    /// The thread it ran in.
    pub thread: ThreadId,
    /// The turn it ran in.
    pub turn: TurnId,
    /// The tool call that ran it.
    pub tool_call_id: ToolCallId,
    /// The agent that ran the search, by registry id.
    pub agent: AgentId,
    /// The query text.
    pub query: String,
    /// The results, in the order the agent got them; empty when there were none.
    pub results: Vec<WebResult>,
    /// When it was recorded.
    pub at: Timestamp,
}

/// One result of a web search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WebResult {
    /// The result's title.
    pub title: String,
    /// The result's address.
    pub url: String,
}

/// A page the agent opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PageVisited {
    /// The thread it ran in.
    pub thread: ThreadId,
    /// The turn it ran in.
    pub turn: TurnId,
    /// The tool call that opened it.
    pub tool_call_id: ToolCallId,
    /// The page's address.
    pub url: String,
    /// The page's title, when the agent reported one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    /// When it was recorded.
    pub at: Timestamp,
    /// How the page was opened.
    pub via: PageVia,
}

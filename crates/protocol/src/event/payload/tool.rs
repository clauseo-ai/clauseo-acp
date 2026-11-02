//! Payloads about a tool call, a web search or page, and what the agent asks the lawyer
//! (permissions, elicitations), logged in the thread's stream.

use serde::{Deserialize, Serialize};

use crate::web::{
    MAX_SEARCH_QUERY_CHARS, MAX_SEARCH_RESULTS, MAX_URL_CHARS, MAX_WEB_TITLE_CHARS, cut_chars,
    logged_url,
};
use crate::{
    AgentId, CompactString, Deliverable, ElicitationId, ElicitationOutcome, ElicitationRequest,
    ExpiryReason, MatterPath, PageVia, PermissionChoice, PermissionOption, PermissionRequestId,
    Principal, TextOrBlob, ToolCallContent, ToolCallId, ToolCallPatch, ToolCallStatus, ToolKind,
    ToolLocation, TurnId,
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

/// A web search the agent ran, with the results it got. Built with [`WebSearchRecorded::new`],
/// which applies the caps; the log's envelope carries the stream (the thread) and the time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WebSearchRecorded {
    /// The turn it ran in.
    pub turn: TurnId,
    /// The tool call that ran it.
    pub tool_call: ToolCallId,
    /// The agent that ran the search, by registry id.
    pub agent: AgentId,
    /// The query text: at most [`MAX_SEARCH_QUERY_CHARS`] characters, cut by
    /// [`WebSearchRecorded::new`].
    pub query: String,
    /// The results, in the order the agent got them; empty when there were none. At most
    /// [`MAX_SEARCH_RESULTS`] of them, kept by [`WebSearchRecorded::new`].
    pub results: Vec<WebResult>,
}

impl WebSearchRecorded {
    /// A search the agent ran. The producer calls this, not a struct literal: it cuts `query` to
    /// [`MAX_SEARCH_QUERY_CHARS`] characters, keeps the first [`MAX_SEARCH_RESULTS`] results and
    /// builds each through [`WebResult::new`] (its title capped, its URL logged by
    /// [`logged_url`]).
    pub fn new(
        turn: TurnId,
        tool_call: ToolCallId,
        agent: AgentId,
        query: &str,
        results: Vec<WebResult>,
    ) -> Self {
        Self {
            turn,
            tool_call,
            agent,
            query: cut_chars(query.to_owned(), MAX_SEARCH_QUERY_CHARS),
            results: results
                .into_iter()
                .take(MAX_SEARCH_RESULTS)
                .map(|result| WebResult::new(result.title, &result.url))
                .collect(),
        }
    }
}

/// One result of a web search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WebResult {
    /// The result's title: at most [`MAX_WEB_TITLE_CHARS`] characters.
    pub title: String,
    /// The result's address: at most [`MAX_URL_CHARS`] characters, after [`logged_url`].
    pub url: String,
}

impl WebResult {
    /// A result. The producer calls this: it cuts `title` to [`MAX_WEB_TITLE_CHARS`] characters
    /// and logs `url` by [`logged_url`], then cuts it to [`MAX_URL_CHARS`].
    pub fn new(title: impl Into<String>, url: &str) -> Self {
        Self {
            title: cut_chars(title.into(), MAX_WEB_TITLE_CHARS),
            url: cut_chars(logged_url(url), MAX_URL_CHARS),
        }
    }
}

/// A page the agent opened. Built with [`PageVisited::new`], which applies the caps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PageVisited {
    /// The turn it ran in.
    pub turn: TurnId,
    /// The tool call that opened it.
    pub tool_call: ToolCallId,
    /// The page's address: at most [`MAX_URL_CHARS`] characters, after [`logged_url`].
    pub url: String,
    /// The page's title, when the agent reported one: at most [`MAX_WEB_TITLE_CHARS`]
    /// characters. Left out of the JSON when there is none.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    /// How the page was opened.
    pub via: PageVia,
}

impl PageVisited {
    /// A page the agent opened. The producer calls this: it logs `url` by [`logged_url`] and cuts
    /// it to [`MAX_URL_CHARS`] characters, and cuts `title` to [`MAX_WEB_TITLE_CHARS`].
    pub fn new(
        turn: TurnId,
        tool_call: ToolCallId,
        url: &str,
        title: Option<String>,
        via: PageVia,
    ) -> Self {
        Self {
            turn,
            tool_call,
            url: cut_chars(logged_url(url), MAX_URL_CHARS),
            title: title.map(|title| cut_chars(title, MAX_WEB_TITLE_CHARS)),
            via,
        }
    }
}

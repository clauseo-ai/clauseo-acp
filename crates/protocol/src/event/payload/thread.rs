//! Payloads about a thread and its agent session, logged in the thread's stream.

use serde::{Deserialize, Serialize};

use crate::{
    AgentCapabilities, AgentId, AgentSessionId, AvailableCommand, CompactString, CompactionStatus,
    ConfigOption, Cost, Count, DetachReason, MatterId, NoticeLevel, PlanEntry, RenameSource,
    TextOrBlob,
};

/// A thread was created in a matter: the first event of its stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadCreated {
    /// The matter it belongs to.
    pub matter: MatterId,
    /// The agent, fixed for the thread's life.
    pub agent: AgentId,
    /// A title, when one was given.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub title: Option<CompactString>,
}

/// The thread got a new title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadRenamed {
    /// The new title.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub title: CompactString,
    /// Who renamed it.
    pub by: RenameSource,
}

/// The thread was archived. JSON: `{"thread_archived": {}}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadArchived {}

/// An agent session started or was loaded for the thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentSessionAttached {
    /// ACP's session id, a foreign attribute.
    pub session: AgentSessionId,
    /// The agent's version, when it reported one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_version: Option<String>,
    /// What the agent can do.
    pub capabilities: AgentCapabilities,
}

/// The thread's agent session ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentSessionDetached {
    /// Why.
    pub reason: DetachReason,
}

/// The agent's plan, replaced whole (ACP v1 sends the full plan each time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PlanReplaced {
    /// The entries.
    pub entries: Vec<PlanEntry>,
}

/// The session's mode changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ModeChanged {
    /// The new mode's id.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub mode_id: CompactString,
}

/// The session's config options, replaced whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConfigOptionsReplaced {
    /// The options.
    pub options: Vec<ConfigOption>,
}

/// The agent's slash commands, replaced whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CommandsAvailable {
    /// The commands.
    pub commands: Vec<AvailableCommand>,
}

/// The agent reported its context use and cost. Token counts are at most
/// [`MAX_SAFE`](crate::MAX_SAFE).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsageUpdated {
    /// Tokens in the context now.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub context_used: Option<Count>,
    /// The context's size in tokens.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub context_size: Option<Count>,
    /// The session's cost so far, shown only to pay-as-you-go accounts.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cost: Option<Cost>,
}

/// A notice for the lawyer, from the agent or the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Notice {
    /// How prominent it is.
    pub level: NoticeLevel,
    /// What it says.
    pub text: String,
}

/// Context compaction started or finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CompactionChanged {
    /// Where it stands.
    pub status: CompactionStatus,
    /// The summary the agent kept, when it sent one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub summary: Option<TextOrBlob>,
}

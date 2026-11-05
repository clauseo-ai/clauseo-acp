//! Payloads about a thread and its agent session, logged in the thread's stream.

use serde::{Deserialize, Serialize};

use crate::{
    AgentCapabilities, AgentId, AgentSessionId, AvailableCommand, CompactString, CompactionStatus,
    ConfigOption, Cost, Count, DetachReason, DoneBy, MatterId, NoticeKind, NoticeLevel, PlanEntry,
    RenameSource, SnoozeBy, TextOrBlob, Timestamp,
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

/// The thread's snooze was set or cleared: by the lawyer until `until`, or cleared by hand
/// (`until` left out), or cleared by the host when its time came (`by: expired`). The reducer
/// keeps it as [`HeaderEntry::snoozed_until`](crate::HeaderEntry); the view needs no clock,
/// since the host logs the wake. A later lawyer's message, permission request, elicitation,
/// limit stop, failed or ended turn clears it too.
/// JSON: `{"thread_snoozed": {"until": 1760000000000, "by": "lawyer"}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadSnoozed {
    /// When the snooze ends; left out when the snooze was cleared.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub until: Option<Timestamp>,
    /// Who set or cleared it.
    pub by: SnoozeBy,
}

/// The thread was marked done (`done: true`) or not done. The reducer keeps it as
/// [`HeaderEntry::done`](crate::HeaderEntry); the same events that clear a snooze clear it.
/// JSON: `{"thread_done": {"done": true, "by": "morning_sweep"}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadDone {
    /// Done, or no longer done.
    pub done: bool,
    /// Who marked it.
    pub by: DoneBy,
}

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

/// The agent's own display name, from `initialize`'s `agentInfo.title`, cleaned for display by
/// the host. It never picks or identifies the agent: the agent's settings id does that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentNamed {
    /// The name to show.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub agent_name: CompactString,
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

/// A notice for the lawyer, from the agent or the host. A host notice carries a `kind` the app
/// shows in its own words; an agent notice never does. `text` is the fallback: what a view that
/// does not know the kind shows, and an agent notice's only words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Notice {
    /// How prominent it is.
    pub level: NoticeLevel,
    /// What it says: the agent's words, or the host's English fallback for its `kind`.
    pub text: String,
    /// Which host notice it is; left out for an agent notice and in logs written before kinds.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub kind: Option<NoticeKind>,
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

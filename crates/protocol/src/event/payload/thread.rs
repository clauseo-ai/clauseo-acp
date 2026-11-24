//! Payloads about a thread and its agent session, logged in the thread's stream.

use serde::{Deserialize, Serialize};

use crate::open_enum::open_unit_enum;
use crate::{
    AgentCapabilities, AgentId, AgentSessionId, AvailableCommand, CompactString, CompactionStatus,
    ConfigOption, Cost, Count, DetachReason, DoneBy, HandoffId, MatterId, MessageId, NamedChoice,
    NoticeKind, NoticeLevel, PlanEntry, RenameSource, SnoozeBy, TextOrBlob, Timestamp, TurnId,
};

/// A thread was created in a matter: the first event of its stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadCreated {
    /// The matter it belongs to.
    pub matter: MatterId,
    /// The agent the thread started with; an `AgentSwitched` changes it.
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

/// The agent said it waits to retry its service in the middle of `turn` (a dropped stream, the
/// model at capacity, an overload), with the agent's own words for it: the host gives such a
/// turn longer silence before it ends it. One per such word of the agent's; each new one stands
/// for the last. The texts are the agent's, each at most 300 characters; the agent says nothing
/// of how long it waits, and Clauseo never reads a delay out of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentRetrying {
    /// The turn it waits in.
    pub turn: TurnId,
    /// The agent's headline, such as `Reconnecting... 1/5`, when it sent one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    /// The agent's cause, such as `HTTP 529 (overloaded). Next attempt in 8s.`, when it sent
    /// one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub details: Option<String>,
}

/// The agent's wait to retry in `turn` is over: it sent something else. The turn's end ends the
/// wait too, with no event of this kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentRetryEnded {
    /// The turn it waited in.
    pub turn: TurnId,
}

/// The thread's agent changed, at the start of the turn of a lawyer's message that named
/// another agent. The thread's next session is the new agent's, started new; the old agent's
/// session detached `switched` before this.
/// JSON: `{"agent_switched": {"from": "claude", "to": "codex"}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentSwitched {
    /// The agent the thread had.
    pub from: AgentId,
    /// The agent it has now.
    pub to: AgentId,
}

/// One end of a handover: an agent, with the model and effort as the thread showed them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Endpoint {
    /// The agent.
    pub agent: AgentId,
    /// The model, from the thread's config options; left out when not known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<NamedChoice>,
    /// The effort, from the thread's config options; left out when not known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effort: Option<NamedChoice>,
}

/// The conversation's earlier items are being handed to the agent a switch moved to, with the
/// lawyer's `message`; a `HandoffSettled` of the same `handoff` says how it went. The
/// counts say what the handover holds: items are lawyer messages, answers, commands, errors and
/// changed papers, each kept whole or left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ContextHandoff {
    /// The handover.
    pub handoff: HandoffId,
    /// The lawyer's message it goes with, which the view shows it before.
    pub message: MessageId,
    /// The agent it comes from.
    pub from: Endpoint,
    /// The agent it goes to.
    pub to: Endpoint,
    /// The earlier answers the conversation holds.
    pub answers_covered: Count,
    /// The earlier items the handover holds.
    pub items_included: Count,
    /// The earlier items it leaves out, for want of room.
    pub items_left_out: Count,
}

/// How a handover went: its prompt went out to the new agent, or the turn failed first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HandoffSettled {
    /// The handover.
    pub handoff: HandoffId,
    /// How it went.
    pub status: HandoffStatus,
}

open_unit_enum! {
    /// How a handover went.
    pub enum HandoffStatus {
        /// The prompt holding it went out to the new agent.
        Delivered,
        /// The turn failed before its prompt went out.
        Failed,
    }
    test mod handoff_status;
}

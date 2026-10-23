//! The agent session behind a thread: what the agent can do, why it went away, and the session
//! state it reports (modes, config options, commands, plan, usage, notices, compaction).
//!
//! The shapes follow ACP v1's stable `session/update` variants (schema 1.11.0), as Clauseo's own
//! types: `protocol` never depends on `agent-client-protocol`.

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};
use crate::{CompactString, Count, Finite};

/// The agent's stable ACP v1 capabilities, plus Clauseo's steering capability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentCapabilities {
    /// The agent replays a session's history on `session/load`.
    pub load_session: bool,
    /// The block kinds its prompts accept besides text and resource links.
    pub prompt: PromptCapabilities,
    /// The session methods it supports.
    pub session: SessionCapabilities,
    /// How it takes a steer, when it can; `None` when it has no `_session/steering`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub steering: Option<SteeringCapability>,
}

/// ACP's `promptCapabilities`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PromptCapabilities {
    /// Image blocks.
    pub image: bool,
    /// Audio blocks.
    pub audio: bool,
    /// Embedded resource blocks.
    pub embedded_context: bool,
}

/// ACP's `sessionCapabilities`: the stable session methods Clauseo uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SessionCapabilities {
    /// `session/list`.
    pub list: bool,
    /// `session/resume`.
    pub resume: bool,
    /// `session/close`.
    pub close: bool,
}

/// How an agent takes a message steered into its running turn. `acp` fills both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SteeringCapability {
    /// The agent advertises `_session/steering`.
    pub native: bool,
    /// A steer that arrives after the turn ended goes back to the host instead of starting a
    /// turn of the agent's own (only `claude-agent-acp`).
    pub idle_fallback_to_host: bool,
}

open_data_enum! {
    /// Why an agent session ended.
    ///
    /// A plain string for a unit variant, a one-key object for `Exited`; a kind from a newer
    /// Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum DetachReason {
        /// The agent process exited.
        Exited = "exited" {
            /// Its exit code; `None` when a signal ended it.
            #[serde(skip_serializing_if = "Option::is_none", default)]
            code: Option<i32>,
            /// The end of its stderr, at most 4 KiB, redacted.
            stderr_tail: String,
        },
        /// Clauseo closed the session.
        Closed = "closed",
        /// Clauseo is restarting the agent.
        Restarting = "restarting",
        /// Clauseo stopped the idle agent to save memory.
        IdleStopped = "idle_stopped",
        _ =>
        /// A reason from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod detach_reason with [
        DetachReason::Exited {
            code: Some(-1),
            stderr_tail: "panic".to_owned(),
        },
        DetachReason::Closed,
        DetachReason::Restarting,
        DetachReason::IdleStopped,
    ];
}

/// A session config option the agent offers (ACP's `session/set_config_option`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConfigOption {
    /// The option's id, sent back in `SetConfigOption`.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub id: CompactString,
    /// Its name.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub name: CompactString,
    /// A description to show.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    /// What it configures.
    pub category: ConfigCategory,
    /// Its type and current value.
    pub kind: ConfigOptionKind,
}

open_unit_enum! {
    /// What a config option configures: ACP's categories plus `Other`. `acp` maps ACP's
    /// missing category to `Other`; a category from a newer Clauseo decodes to `Unknown`.
    pub enum ConfigCategory {
        /// The session mode.
        Mode,
        /// The model.
        Model,
        /// How much the model thinks.
        ThoughtLevel,
        /// Anything else.
        Other,
    }
    test mod config_category;
}

open_data_enum! {
    /// A config option's type and current value.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ConfigOptionKind {
        /// One value out of a list.
        Select = "select" {
            /// The current value's id.
            #[cfg_attr(feature = "specta", specta(type = String))]
            current_value: CompactString,
            /// The values to choose from, ACP's groups flattened in order.
            options: Vec<ConfigChoice>,
        },
        /// On or off.
        Boolean = "boolean" {
            /// The current value.
            current_value: bool,
        },
        _ =>
        /// A kind from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod config_option_kind with [
        ConfigOptionKind::Select {
            current_value: CompactString::const_new("fast"),
            options: vec![ConfigChoice {
                value: CompactString::const_new("fast"),
                name: CompactString::const_new("Fast"),
                description: Some("Quicker answers".to_owned()),
                group: None,
            }],
        },
        ConfigOptionKind::Boolean {
            current_value: true,
        },
    ];
}

/// One value of a `Select` config option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConfigChoice {
    /// The value's id, sent back in `SetConfigOption`.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub value: CompactString,
    /// Its name.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub name: CompactString,
    /// A description to show.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    /// The name of the ACP group it came from, if the agent grouped its values.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub group: Option<String>,
}

/// A slash command the agent offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AvailableCommand {
    /// Its name, without the slash.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub name: CompactString,
    /// What it does.
    pub description: String,
    /// A hint for its input, when it takes one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub input_hint: Option<String>,
}

/// One entry of the agent's plan. ACP v1 sends the full plan each time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PlanEntry {
    /// What the entry says.
    pub content: String,
    /// How important it is.
    pub priority: PlanPriority,
    /// Where it stands.
    pub status: PlanStatus,
}

open_unit_enum! {
    /// A plan entry's priority.
    pub enum PlanPriority {
        /// High.
        High,
        /// Medium.
        Medium,
        /// Low.
        Low,
    }
    test mod plan_priority;
}

open_unit_enum! {
    /// A plan entry's status.
    pub enum PlanStatus {
        /// Not started.
        Pending,
        /// Being worked on.
        InProgress,
        /// Done.
        Completed,
    }
    test mod plan_status;
}

/// The session's cost so far, as the agent reports it (an estimate at list prices).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Cost {
    /// The amount, in `currency`.
    pub amount: Finite,
    /// The ISO 4217 currency code, such as `USD`.
    pub currency: String,
}

/// Token counts for one turn, when the agent reports them. Each count is at most
/// [`MAX_SAFE`](crate::MAX_SAFE).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnUsage {
    /// Every token the turn used.
    pub total: Count,
    /// Input tokens.
    pub input: Count,
    /// Output tokens.
    pub output: Count,
    /// Thinking tokens, when reported apart.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thought: Option<Count>,
    /// Input tokens read from the agent's cache.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cached_read: Option<Count>,
    /// Input tokens written to the agent's cache.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cached_write: Option<Count>,
}

open_unit_enum! {
    /// How prominent a notice is.
    pub enum NoticeLevel {
        /// For information.
        Info,
        /// Something the lawyer should look at.
        Warning,
    }
    test mod notice_level;
}

open_unit_enum! {
    /// Where context compaction stands.
    pub enum CompactionStatus {
        /// The agent started compacting its context.
        Started,
        /// It finished.
        Completed,
    }
    test mod compaction_status;
}

open_unit_enum! {
    /// Who renamed a thread.
    pub enum RenameSource {
        /// The lawyer.
        Lawyer,
        /// The agent (`session_info_update`).
        Agent,
    }
    test mod rename_source;
}

open_unit_enum! {
    /// Which stream of an agent's message a chunk belongs to.
    pub enum TextKind {
        /// The answer.
        Answer,
        /// The agent's thinking.
        Thought,
    }
    test mod text_kind;
}

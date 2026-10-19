//! The agent session behind a thread: what the agent can do, why it went away, and the session
//! state it reports (modes, config options, commands, plan, usage, notices, compaction).
//!
//! The shapes follow ACP v1's stable `session/update` variants (schema 1.11.0), as Clauseo's own
//! types: `protocol` never depends on `agent-client-protocol`.

use serde::de::MapAccess;
use serde::{Deserialize, Deserializer, Serialize};

use crate::CompactString;
use crate::open_enum::{
    OpenDataEnum, deserialize_open, enum_with_all, open_unit_enum, serialize_kind,
};

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

/// How an agent takes a message steered into its running turn. `acp` (R10) fills both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SteeringCapability {
    /// The agent advertises `_session/steering`.
    pub native: bool,
    /// A steer that arrives after the turn ended goes back to the host instead of starting a
    /// turn of the agent's own (only `claude-agent-acp`).
    pub idle_fallback_to_host: bool,
}

enum_with_all! {
    /// Why an agent session ended. M1 emits `Exited` and `Closed`, M2 the other two.
    ///
    /// A plain string for a unit variant, a one-key object for `Exited`; a kind from a newer
    /// Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "specta", derive(specta::Type))]
    pub enum DetachReason {
        /// The agent process exited.
        Exited {
            /// Its exit code; `None` when a signal ended it.
            #[serde(skip_serializing_if = "Option::is_none", default)]
            code: Option<i32> = None,
            /// The end of its stderr, at most 4 KiB, redacted.
            stderr_tail: String = String::new(),
        },
        /// Clauseo closed the session.
        Closed,
        /// Clauseo is restarting the agent.
        Restarting,
        /// Clauseo stopped the idle agent to save memory.
        IdleStopped,
        /// A reason from a newer Clauseo; written back as the plain string `kind`.
        #[serde(untagged, serialize_with = "serialize_kind::<DetachReason, _>")]
        #[cfg_attr(feature = "specta", specta(type = String))]
        Unknown {
            /// The reason's key as read.
            kind: String = String::new(),
        },
    }
}

#[derive(Deserialize)]
struct ExitedData {
    #[serde(default)]
    code: Option<i32>,
    stderr_tail: String,
}

impl OpenDataEnum for DetachReason {
    const EXPECTING: &'static str = "a detach reason";
    const DATA_KINDS: &'static [&'static str] = &["exited"];

    fn unit(kind: &str) -> Option<Self> {
        match kind {
            "closed" => Some(Self::Closed),
            "restarting" => Some(Self::Restarting),
            "idle_stopped" => Some(Self::IdleStopped),
            _ => None,
        }
    }

    fn data<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Option<Self>, A::Error> {
        Ok(match kind {
            "exited" => {
                let ExitedData { code, stderr_tail } = map.next_value()?;
                Some(Self::Exited { code, stderr_tail })
            }
            _ => None,
        })
    }

    fn unknown(kind: &str) -> Self {
        Self::Unknown {
            kind: kind.to_owned(),
        }
    }
}

impl<'de> Deserialize<'de> for DetachReason {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_open(deserializer)
    }
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

/// What a config option configures: ACP's categories plus `Other`. The enum is closed, so a
/// string it does not list fails to decode; `acp` (R10) maps ACP's missing or unknown category to
/// `Other` before the event is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
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

/// A config option's type and current value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ConfigOptionKind {
    /// One value out of a list.
    Select {
        /// The current value's id.
        #[cfg_attr(feature = "specta", specta(type = String))]
        current_value: CompactString,
        /// The values to choose from, ACP's groups flattened in order.
        options: Vec<ConfigChoice>,
    },
    /// On or off.
    Boolean {
        /// The current value.
        current_value: bool,
    },
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

/// A plan entry's priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PlanPriority {
    /// High.
    High,
    /// Medium.
    Medium,
    /// Low.
    Low,
}

/// A plan entry's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PlanStatus {
    /// Not started.
    Pending,
    /// Being worked on.
    InProgress,
    /// Done.
    Completed,
}

/// The session's cost so far, as the agent reports it (an estimate at list prices).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Cost {
    /// The amount, in `currency`.
    pub amount: f64,
    /// The ISO 4217 currency code, such as `USD`.
    pub currency: String,
}

/// Token counts for one turn, when the agent reports them. Each count is at most
/// [`MAX_SAFE`](crate::MAX_SAFE).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TurnUsage {
    /// Every token the turn used.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    #[serde(with = "crate::number::safe")]
    pub total: u64,
    /// Input tokens.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    #[serde(with = "crate::number::safe")]
    pub input: u64,
    /// Output tokens.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    #[serde(with = "crate::number::safe")]
    pub output: u64,
    /// Thinking tokens, when reported apart.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    #[serde(with = "crate::number::safe_opt")]
    pub thought: Option<u64>,
    /// Input tokens read from the agent's cache.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    #[serde(with = "crate::number::safe_opt")]
    pub cached_read: Option<u64>,
    /// Input tokens written to the agent's cache.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    #[serde(with = "crate::number::safe_opt")]
    pub cached_write: Option<u64>,
}

open_unit_enum! {
    /// How prominent a notice is.
    pub enum NoticeLevel {
        /// For information.
        Info,
        /// Something the lawyer should look at.
        Warning,
    }
}

/// Where context compaction stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum CompactionStatus {
    /// The agent started compacting its context.
    Started,
    /// It finished.
    Completed,
}

open_unit_enum! {
    /// Who renamed a thread.
    pub enum RenameSource {
        /// The lawyer.
        Lawyer,
        /// The agent (`session_info_update`).
        Agent,
    }
}

/// Which stream of an agent's message a chunk belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum TextKind {
    /// The answer.
    Answer,
    /// The agent's thinking.
    Thought,
}

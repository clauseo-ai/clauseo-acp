//! What the driver sends the host: [`AcpEvent`] and the types inside it.
//!
//! Payloads use `protocol`'s types wherever it has one. Agent ids (`toolCallId`, `messageId`,
//! session ids) stay the agent's text: `host` mints Clauseo's ids and dedupes on them.

use clauseo_protocol::{
    AgentSessionId, AvailableCommand, CompactString, ConfigOption, ContentBlock, Cost, Count,
    ElicitationRequest, Finite, LimitStopReason, NoticeLevel, PermissionOption, PlanEntry,
    TextOrBlob, Timestamp, ToolCallContent, ToolCallPatch, ToolCallStatus, ToolKind, ToolLocation,
    TurnId,
};

/// One event from the agent connection. Not `#[non_exhaustive]`: a new variant breaks `host`'s
/// match until it handles it.
#[derive(Debug, Clone, PartialEq)]
pub enum AcpEvent {
    /// A `session/update` the driver mapped.
    Update {
        /// The agent's session.
        session: AgentSessionId,
        /// When it came, relative to the session's turns.
        origin: Origin,
        /// What it says.
        update: AgentUpdate,
    },
    /// The agent asks for permission. Answer with `AcpClient::answer_permission`.
    PermissionRequested {
        /// The agent's session.
        session: AgentSessionId,
        /// The key to answer it with.
        key: PendingKey,
        /// The agent's `toolCallId` the request is about.
        acp_tool_call_id: CompactString,
        /// The options the agent offered, in its order.
        options: Vec<PermissionOption>,
    },
    /// The agent asks the lawyer for input. Answer with `AcpClient::answer_elicitation`.
    ElicitationRequested {
        /// The agent's session.
        session: AgentSessionId,
        /// The key to answer it with.
        key: PendingKey,
        /// The agent's `elicitationId`, which URL elicitations carry and
        /// [`AcpEvent::ElicitationCompleted`] names again; `None` for a form.
        elicitation_id: Option<String>,
        /// The form or the URL.
        request: ElicitationRequest,
    },
    /// The agent says a URL elicitation finished (`elicitation/complete`).
    ElicitationCompleted {
        /// The agent's `elicitationId`.
        elicitation_id: String,
    },
    /// A plan limit signal from an update (`_claude/rateLimit` on `usage_update`, or the AIR
    /// `sessionFailure` on `session_info_update`).
    Limit {
        /// The agent's session.
        session: AgentSessionId,
        /// When it came.
        origin: Origin,
        /// What it says.
        signal: LimitSignal,
    },
    /// Something the agent sent that the driver skipped or did not understand.
    Warning(ProtocolWarning),
}

/// When an update came, relative to the session's turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// While the prompt of this turn is in flight.
    Turn(TurnId),
    /// From `cancel()` until the next `prompt()` starts, whether or not the cancelled prompt's
    /// response has come. The host drops these by turn.
    Late(TurnId),
    /// Between sending `session/load` and its response: history the agent replays.
    Replay,
    /// Any other time, such as between turns.
    Idle,
}

/// A per-connection key for a request the agent sent (a permission or an elicitation). The host
/// maps it to its own `PermissionRequestId` or `ElicitationId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PendingKey(pub u64);

/// Who a message chunk is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageRole {
    /// The lawyer's message, as the agent replays it.
    User,
    /// The agent's answer.
    Answer,
    /// The agent's visible reasoning.
    Thought,
}

/// One stable `session/update` variant of schema 1.11.0, in `protocol`'s types.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentUpdate {
    /// `user_message_chunk`, `agent_message_chunk`, `agent_thought_chunk`. A text block over
    /// 1 MiB comes as several chunks with the same `acp_message_id`.
    MessageChunk {
        /// Whose message.
        role: MessageRole,
        /// The agent's `messageId`, when it sent one.
        acp_message_id: Option<String>,
        /// The block. An ACP block type the driver does not know is `Unsupported`.
        content: ContentBlock,
    },
    /// `tool_call`: a new tool call.
    ToolCall(ToolCall),
    /// `tool_call_update`: the fields it names replace the call's; `content` replaces the list
    /// wholesale.
    ToolCallUpdate {
        /// The agent's `toolCallId`.
        acp_id: CompactString,
        /// The changed fields.
        patch: ToolCallPatch,
    },
    /// `plan`: the whole plan, each time.
    Plan(Vec<PlanEntry>),
    /// `available_commands_update`.
    Commands(Vec<AvailableCommand>),
    /// `current_mode_update`.
    CurrentMode {
        /// The agent's mode id.
        mode_id: CompactString,
    },
    /// `config_option_update`: every option, each time.
    ConfigOptions(Vec<ConfigOption>),
    /// `session_info_update` with a `title`: `None` when the agent cleared it.
    SessionTitle {
        /// The title.
        title: Option<String>,
    },
    /// `usage_update`.
    Usage {
        /// Tokens in the context window.
        used: Count,
        /// The window's size in tokens.
        size: Count,
        /// The session's cost so far, when the agent reports it.
        cost: Option<Cost>,
    },
    /// `notice`: the title, and the description after a blank line when there is one.
    Notice {
        /// `info`, or `warning` (also for the agent's `error`).
        level: NoticeLevel,
        /// The text.
        text: String,
    },
    /// `compaction_update` with status `in_progress`.
    CompactionStarted {
        /// The agent's `compactionId`.
        acp_id: String,
    },
    /// `compaction_update` with status `completed`, `failed` or `cancelled`.
    CompactionEnded {
        /// The agent's `compactionId`.
        acp_id: String,
        /// Whether it completed.
        completed: bool,
    },
    /// `compaction_summary_chunk`: one block of the summary.
    CompactionSummaryChunk {
        /// The agent's `compactionId`.
        acp_id: String,
        /// The block.
        content: ContentBlock,
    },
}

/// A new tool call, as `protocol`'s `ToolCallStarted` holds it, without Clauseo's ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// The agent's `toolCallId`.
    pub acp_id: CompactString,
    /// What the agent says it does.
    pub title: CompactString,
    /// Its kind.
    pub kind: ToolKind,
    /// The agent's tool `name`, by which the UI looks up how to render it.
    pub tool_name: Option<CompactString>,
    /// Its status.
    pub status: ToolCallStatus,
    /// The places it touches, inside the session's `cwd` or flagged as outside.
    pub locations: Vec<ToolLocation>,
    /// Its input as compact JSON text.
    pub raw_input: Option<TextOrBlob>,
    /// Its output as compact JSON text.
    pub raw_output: Option<TextOrBlob>,
    /// Its content.
    pub content: Vec<ToolCallContent>,
}

/// A plan limit signal: Claude's rate limit, or an AIR `sessionFailure` that stopped the turn.
#[derive(Debug, Clone, PartialEq)]
pub enum LimitSignal {
    /// `_claude/rateLimit` on a `usage_update`. A missing or unreadable field is `None`.
    RateLimit {
        /// `rateLimitType`, such as `five_hour` or `seven_day`.
        window: Option<String>,
        /// `status`.
        status: LimitStatus,
        /// `utilization` times 100.
        used_percent: Option<Finite>,
        /// `resetsAt` (epoch seconds) in milliseconds.
        resets_at: Option<Timestamp>,
    },
    /// An AIR `sessionFailure` of category `limit` and severity `error`: no actions is the plan
    /// window (`quota_exhausted`), `["retry"]` a temporary limit (`rate_limited`).
    Stopped {
        /// Why.
        reason: LimitStopReason,
        /// The failure's `title`, for display.
        title: String,
    },
}

/// The `status` of Claude's rate limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitStatus {
    /// `allowed`.
    Allowed,
    /// `allowed_warning`: close to the limit.
    Warning,
    /// `rejected`: at the limit.
    Rejected,
}

/// Something the agent sent that the driver skipped. None of these ends the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolWarning {
    /// A stdout line that is not UTF-8 JSON; nothing was written back to the agent.
    MalformedLine {
        /// Its length in bytes.
        bytes: usize,
    },
    /// A `session/update` whose `sessionUpdate` the driver does not map.
    UnknownUpdate {
        /// The `sessionUpdate` name.
        name: String,
    },
    /// A known `session/update` that failed to decode, or held an id no command could name.
    InvalidUpdate {
        /// The `sessionUpdate` name.
        name: String,
        /// Why.
        error: String,
    },
    /// `_claude/rateLimit` with a `status` other than `allowed`, `allowed_warning`, `rejected`.
    UnknownLimitStatus {
        /// The status.
        status: String,
    },
    /// An AIR `sessionFailure` that is not a plan limit (`context_exhausted`, a warning, ...).
    SessionFailure {
        /// Its `category`.
        category: String,
        /// Its `severity`.
        severity: String,
        /// Its `title`.
        title: String,
    },
}

/// One JSON-RPC line, copied to `ConnectOptions::frame_tap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    /// Which way it went.
    pub direction: FrameDirection,
    /// The line, without its newline.
    pub line: String,
}

/// Which way a [`RawFrame`] went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameDirection {
    /// From the agent.
    In,
    /// To the agent.
    Out,
}

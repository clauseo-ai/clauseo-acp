//! What the driver sends the host: [`AcpEvent`] and the types inside it.
//!
//! Payloads use `protocol`'s types wherever it has one. Agent ids stay the agent's text: a
//! session id is an `AgentSessionId`, every other agent id (`toolCallId`, `messageId`,
//! `compactionId`, `elicitationId`, an auth method's `id`) a `CompactString`. `host` mints
//! Clauseo's ids and dedupes on them.

use std::path::PathBuf;

use clauseo_protocol::{
    AgentSessionId, AvailableCommand, CompactString, ConfigOption, ContentBlock, Cost, Count,
    ElicitationRequest, Finite, HelperOutcome, LimitStopReason, NoticeKind, NoticeLevel,
    PermissionOption, PlanEntry, TextOrBlob, Timestamp, ToolCallContent, ToolCallPatch,
    ToolCallStatus, ToolKind, ToolLocation, TurnId,
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
    /// The same as [`AcpEvent::Update`], for an update that belongs to the helper (a subagent
    /// the lead agent launched) the agent names `helper`. claude-agent-acp stamps a helper's
    /// updates with `_meta.claudeCode.parentToolUseId` (`dist/sdk-to-acp.js:9-17`); that stamp,
    /// when it is a non-empty string, is `helper`. A recorded 0.110.1 run stamped each of the
    /// helper's `tool_call`s but none of their `tool_call_update`s, which therefore come as
    /// `Update`: the host names a step's helper from its `tool_call`.
    ///
    /// codex-acp, on a connection with
    /// [`ConnectOptions::helper_sessions`](crate::ConnectOptions::helper_sessions), sends a
    /// helper's updates on the helper's own session id: each update of a helper announced by
    /// [`AcpEvent::HelperSpawned`] maps as it would on the root session and comes as this, on
    /// the root session; a `final_answer` chunk comes as [`AcpEvent::HelperAnswer`] instead.
    HelperUpdate {
        /// The agent's session: the lead's, which carries the helper's updates; for codex-acp,
        /// the root session.
        session: AgentSessionId,
        /// When it came, relative to the session's turns (for codex-acp, the root's).
        origin: Origin,
        /// The helper: for Claude, the `toolCallId` of the lead's `Agent` call that launched it;
        /// for Codex, the helper's session id.
        helper: CompactString,
        /// What it says.
        update: AgentUpdate,
    },
    /// The agent says the call that launched helper `helper` returns at once and the helper
    /// keeps working: the call's completion is not the helper's end. claude-agent-acp says so
    /// with `_meta.claudeCode.toolResponse.isAsync: true` on a `tool_call_update` of the launch
    /// (`dist/tool-calls/content.js:198-208`, `dist/tool-calls/renderer.js:425-436`). Comes
    /// before the event that same update maps to.
    HelperInBackground {
        /// The agent's session.
        session: AgentSessionId,
        /// When it came, relative to the session's turns.
        origin: Origin,
        /// The helper: the update's `toolCallId`, the launching call's.
        helper: CompactString,
    },
    /// codex-acp launched helper `helper` (its `subagent_spawned`). Only on a connection with
    /// [`ConnectOptions::helper_sessions`](crate::ConnectOptions::helper_sessions). The helper's
    /// own updates then come on its own session id, and reach the host as
    /// [`AcpEvent::HelperUpdate`] and [`AcpEvent::HelperAnswer`] on `session`.
    HelperSpawned {
        /// The session the lawyer talks to (the root), whichever session the frame came on.
        session: AgentSessionId,
        /// When it came: the root session's origin.
        origin: Origin,
        /// The helper: its `subagentSessionId`, the session id its own updates carry.
        helper: CompactString,
        /// The helper that launched it, when the frame came on a helper's session; `None` when
        /// the lead launched it.
        parent: Option<CompactString>,
        /// Its `name`: codex-acp turns the model's task name into words (`nameFromAgentPath`,
        /// `dist/index.js:37928`).
        name: CompactString,
        /// Its `task`; `None` when empty or the filler `Delegated task for <name>` codex-acp
        /// sends when it has no text (`dist/index.js:37932`), as under Codex's multi-agent v2.
        task: Option<CompactString>,
    },
    /// A piece of helper `helper`'s report: an `agent_message_chunk` on the helper's session
    /// with `_meta.jetbrains.air.phase` `final_answer`. Its other chunks (`commentary`, or no
    /// phase) come as [`AcpEvent::HelperUpdate`]. A text over 1 MiB comes as several.
    HelperAnswer {
        /// The root session.
        session: AgentSessionId,
        /// When it came: the root session's origin.
        origin: Origin,
        /// The helper's session id.
        helper: CompactString,
        /// The block.
        content: ContentBlock,
    },
    /// codex-acp says helper `helper` ended (`subagent_state_update`). Comes once per
    /// announced helper, and only for a helper [`AcpEvent::HelperSpawned`] announced under the
    /// same root; any other end, and one with an unknown state, is a
    /// [`ProtocolWarning::InvalidUpdate`]. After it, whatever the state, the helper's updates
    /// come [`Origin::Late`] with the turn it ended in, or are dropped with a warning when it
    /// ended outside a turn; its requests are answered `cancelled` with a warning, never sent to
    /// the host; and a helper it announces is refused with a warning.
    HelperEnded {
        /// The root session.
        session: AgentSessionId,
        /// When it came: the root session's origin.
        origin: Origin,
        /// The helper's session id.
        helper: CompactString,
        /// How it ended.
        state: HelperState,
    },
    /// The agent says the task of helper `helper` settled: the helper ended with `outcome`.
    /// claude-agent-acp passes Claude Code's task frames on as `_claude/sdkMessage` when the
    /// session's `_meta.claudeCode.emitRawSDKMessages` names them (`map/task.rs`): a
    /// `task_notification` names the launching call, and a `task_updated` with a settle status
    /// names the task, which its `task_started` tied to that call. Both may come for one
    /// settle, so the host takes the first. `helper` may name a call that launched no helper
    /// (a shell command runs as a task too): the host drops it. Nothing else of the frame
    /// comes with it.
    HelperSettled {
        /// The agent's session.
        session: AgentSessionId,
        /// When it came, relative to the session's turns.
        origin: Origin,
        /// The helper: the `toolCallId` of the call that launched it.
        helper: CompactString,
        /// How it ended: `completed` is `Done`, `failed` `Failed`, `stopped` and `killed`
        /// `Stopped`; a `task_notification` status it does not know is `Failed`.
        outcome: HelperOutcome,
    },
    /// The agent asks for permission. Answer with `AcpClient::answer_permission`.
    PermissionRequested {
        /// The agent's session; for a request under the session id of a helper announced by
        /// [`AcpEvent::HelperSpawned`], the root session, and `AcpClient::cancel` of the root
        /// answers it too.
        session: AgentSessionId,
        /// When it came, relative to the session's turns: `Idle` for a turn the agent started
        /// by itself, with no prompt in flight.
        origin: Origin,
        /// The key to answer it with.
        key: PendingKey,
        /// The agent's `toolCallId` the request is about.
        acp_tool_call_id: CompactString,
        /// The options the agent offered, in its order.
        options: Vec<PermissionOption>,
        /// The paths the request names: every `toolCall.locations[].path`, every diff's
        /// `path`, then `rawInput.file_path` and `rawInput.notebook_path`, when they are
        /// strings, without repeats, as the agent wrote them (absolute, or relative to the
        /// session's `cwd`). Empty when it names none.
        paths: Vec<PathBuf>,
        /// `toolCall.rawInput` as the agent sent it, when it did: the host reads the paths
        /// and the shell command in it for its data-folder check (`agent/data_fence.rs`).
        raw_input: Option<serde_json::Value>,
        /// The helper that asks: its session id, when the request came under the session id of
        /// a helper announced by [`AcpEvent::HelperSpawned`]; `None` for the lead's.
        helper: Option<CompactString>,
    },
    /// The agent asks the lawyer for input. Answer with `AcpClient::answer_elicitation`.
    ElicitationRequested {
        /// The agent's session; the root session for a helper's, as for `PermissionRequested`.
        session: AgentSessionId,
        /// When it came, as for `PermissionRequested`.
        origin: Origin,
        /// The key to answer it with.
        key: PendingKey,
        /// The agent's `elicitationId`, which URL elicitations carry and
        /// [`AcpEvent::ElicitationCompleted`] names again; `None` for a form.
        elicitation_id: Option<CompactString>,
        /// The scope's `toolCallId`: the agent's tool call the elicitation belongs to, when it
        /// names one. In-memory only: the host scopes the withheld values to that call
        /// (0002, `elicitation_resolved`); it is not logged.
        acp_tool_call_id: Option<CompactString>,
        /// The form or the URL.
        request: ElicitationRequest,
        /// The helper that asks, as for `PermissionRequested`.
        helper: Option<CompactString>,
    },
    /// The agent says a URL elicitation finished (`elicitation/complete`).
    ElicitationCompleted {
        /// The agent's `elicitationId`.
        elicitation_id: CompactString,
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
    /// An AIR `sessionFailure` on a `session_info_update` that says the turn was too long to
    /// read. The prompt's answer usually carries the same record (`PromptEnd::too_long`); this
    /// keeps a failure sent only on the update from being lost.
    TooLong {
        /// The agent's session.
        session: AgentSessionId,
        /// When it came.
        origin: Origin,
        /// How it said so.
        kind: crate::TooLong,
    },
    /// An AIR `sessionFailure` of category `connection` on a `session_info_update`: the agent
    /// could not reach its service (codex-acp's `transport_lost`), and retries or gave up.
    ConnectionLost {
        /// The agent's session.
        session: AgentSessionId,
        /// When it came.
        origin: Origin,
        /// What it says.
        lost: ConnectionLost,
    },
    /// Something the agent sent that the driver skipped or did not understand.
    Warning {
        /// The agent's session it is about, when the message named one: `None` for a malformed
        /// line, an `elicitation/complete`, or a `session/update` without a usable `sessionId`.
        session: Option<AgentSessionId>,
        /// What it was.
        warning: ProtocolWarning,
    },
    /// A `session/update` over the line cap that the reader dropped, which was or may have been
    /// a `tool_call_update` and whose start showed no status the driver could pass on in its
    /// place. The host completes that tool call, if still unfinished, when the turn ends well;
    /// with no `acp_tool_call_id`, every unfinished tool call of the turn.
    UpdateSkipped {
        /// The agent's session, when the start showed it.
        session: Option<AgentSessionId>,
        /// When it came: `Idle` when the start showed no session.
        origin: Origin,
        /// The agent's `toolCallId`, when the start showed it and its session.
        acp_tool_call_id: Option<CompactString>,
    },
}

/// When an update came, relative to the session's turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// While the prompt of this turn is in flight.
    Turn(TurnId),
    /// From `cancel()` until the cancelled prompt's response comes (or the next `prompt()`
    /// starts). The host drops these by turn.
    Late(TurnId),
    /// Between sending `session/load` and its response: history the agent replays.
    Replay,
    /// Any other time, such as between turns.
    Idle,
}

/// How a helper ended: the `state` of codex-acp's `subagent_state_update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelperState {
    /// `completed`.
    Completed,
    /// `failed`: codex-acp sends it when its wait for open helpers times out, or the turn fails.
    Failed,
    /// `cancelled`: after `session/cancel` on the root. codex-acp then forwards nothing more of
    /// the helper.
    Cancelled,
    /// `disconnected`: replayed by `session/load` for a helper of an earlier process.
    Disconnected,
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
        acp_message_id: Option<CompactString>,
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
        /// The kind when the mapping or the host knows one, else `None`.
        kind: Option<NoticeKind>,
    },
    /// `compaction_update` with status `in_progress`.
    CompactionStarted {
        /// The agent's `compactionId`.
        acp_id: CompactString,
    },
    /// `compaction_update` with status `completed`, `failed` or `cancelled`.
    CompactionEnded {
        /// The agent's `compactionId`.
        acp_id: CompactString,
        /// Whether it completed.
        completed: bool,
    },
    /// `compaction_summary_chunk`: one block of the summary.
    CompactionSummaryChunk {
        /// The agent's `compactionId`.
        acp_id: CompactString,
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
        /// The failure's `title`, for display, when it is a string.
        title: Option<String>,
    },
}

/// An AIR `sessionFailure` of category `connection`: the agent lost its connection to its
/// service. codex-acp sends one with severity `warning` for each retry Codex makes by itself,
/// and one with severity `error` when its retries ran out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionLost {
    /// Its `severity` is `error`: the agent gave up. Anything else, it retries by itself.
    pub gave_up: bool,
    /// Its `title`, the agent's own error text, when it is a string. Never logged; shown only
    /// through [`ConnectionLost::retry`], cut.
    pub title: Option<String>,
    /// What the agent says while it waits to retry: `Some` exactly when `gave_up` is false.
    pub retry: Option<Retrying>,
}

/// The longest `title` or `details` of a [`Retrying`], in characters: longer text is cut on a
/// character boundary.
pub const RETRY_TEXT_CHARS: usize = 300;

/// What an agent says as it waits to retry its service: an AIR `sessionFailure` of severity
/// `warning` in category `connection` (a dropped stream) or `service` (the model at capacity,
/// an overload). codex-acp 2.10.0 titles it `Reconnecting... 1/5` with the cause as its
/// `details`; claude-agent-acp 0.110.1 `Retrying Claude, attempt 2 of 10.` with details such
/// as `HTTP 529 (overloaded). Next attempt in 8s.`. No field carries the delay, which can be
/// minutes. Each text is the agent's own, at most [`RETRY_TEXT_CHARS`] characters, for the
/// lawyer to read; never logged, and never parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retrying {
    /// Its `title`, when it is a string.
    pub title: Option<String>,
    /// Its `details`, when it is a string.
    pub details: Option<String>,
}

/// An AIR `sessionFailure` of severity `error` whose `actions` offer `retry` or `new_session`,
/// in a category no other rule reads (not `limit` nor `connection`, and not too long): the
/// agent gave up on the turn. codex-acp 2.2.2 sends it for `internal_error` (category `service`,
/// "Codex encountered an internal error."), `provider_error` and `overloaded`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFailed {
    /// Its `category`, when it is a string: the agent's word, for an allowlisted log.
    pub category: Option<String>,
    /// Its `title`, the agent's own error text, when it is a string. Never shown or logged.
    pub title: Option<String>,
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
        /// The `sessionUpdate` name, as the agent wrote it.
        name: String,
    },
    /// A `session/update` or another notification that failed to decode: no `update`, no
    /// `sessionUpdate` name, a known update whose fields do not decode, or an id no command
    /// could name. Also a helper's `subagent_*` update this crate refused, and an ended
    /// helper's update or request it dropped or answered itself (see [`AcpEvent::HelperEnded`]).
    InvalidUpdate {
        /// The `sessionUpdate` name, or the notification's method when it is not a
        /// `session/update`; `None` when the update has no `sessionUpdate` string.
        name: Option<String>,
        /// Why.
        error: String,
    },
    /// `_claude/rateLimit` with a `status` other than `allowed`, `allowed_warning`, `rejected`.
    UnknownLimitStatus {
        /// The status; `None` when it is missing or not a string.
        status: Option<String>,
    },
    /// An AIR `sessionFailure` that is neither a plan limit nor a lost connection
    /// (`context_exhausted`, a limit's warning, ...).
    /// Each field is `None` when it is missing or not a string.
    SessionFailure {
        /// Its `category`.
        category: Option<String>,
        /// Its `severity`.
        severity: Option<String>,
        /// Its `title`.
        title: Option<String>,
    },
    /// An AIR `sessionFailure` of category `service` and severity `warning`: the agent waits to
    /// retry its service (Claude's overload retries, Codex's model at capacity). Not a fault of
    /// the agent's: it rides with the warnings, about the session but not its turn, so the host
    /// reads it for the live turn. A `connection` one is a [`ConnectionLost`] instead.
    Retrying(Retrying),
    /// An MCP server that did not start, as codex-acp reports it: a failed `tool_call` titled
    /// `mcp__<name>__startup` (`map/mcp_startup.rs`), which never becomes a step; or as Claude
    /// reports it: a `notice` titled `MCP server "<name>" failed to start`, one warning per
    /// server it names (`map/claude_mcp.rs`). The agent then runs without that server's tools.
    McpServerNotStarted {
        /// The server's name, as the session sent it.
        server: String,
        /// Why, from the adapter's own words; the rest of its text is not kept.
        reason: McpStartupReason,
    },
}

/// Why an MCP server did not start ([`ProtocolWarning::McpServerNotStarted`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum McpStartupReason {
    /// It failed to start for any other reason (in a recorded run, a program that exited at once).
    Failed,
    /// Codex's error says it timed out (in a recorded run, a server that never answered: 30 s).
    TimedOut,
    /// Its start-up was cancelled.
    Cancelled,
    /// codex-acp did not start it: Codex's own config has a server of that name.
    NameTaken,
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

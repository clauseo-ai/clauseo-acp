//! The model refused a turn as too long to read: the question by itself, or the conversation
//! with it. Every shape that says so is read here and nowhere else, so a live recording changes
//! only this file and its scenario. None of it is a plan limit: it never becomes a
//! [`LimitSignal`](crate::LimitSignal), so it never reaches the limits store nor the Codex
//! limits read.
//!
//! The shapes (recordings in the private `measurements/long-question/recordings/`, 2026-11-11):
//! - claude-agent-acp 0.89.1, recorded (`claude-overcap-reply.jsonl`, a 6,000,000-character
//!   question): `end_turn` with a `sessionFailure` of category `service`, severity `error`,
//!   title "Prompt is too long", actions `["retry"]`, after a failed compaction and no answer
//!   text. From its source, the same title can come under category `request` (an API
//!   `invalid_request`), so both categories are read, keyed on the title.
//! - codex-acp 2.2.2, a full context, from source, not recorded (the run was skipped to avoid
//!   billing): Codex's `contextWindowExceeded` becomes `context_exhausted`, sent to an AIR client
//!   as `end_turn` with a `sessionFailure` of category `limit`, severity `error`, actions
//!   `["new_session"]`. Claude's own `context_exhausted` (its configured turn or output limit) has
//!   the same shape: both read as `TooLong::ContextExhausted`, which the host stops on only for
//!   codex-acp.
//! - codex-acp 2.2.2, a question over Codex's 1,048,576 characters, recorded
//!   (`codex-overcap-reply.jsonl`): `end_turn` with a `sessionFailure` of category `service`,
//!   title "Codex encountered an internal error.", actions `["retry", "new_session"]`. The length
//!   reason is only on codex-acp's stderr, so the wire cannot tell it from a real internal error,
//!   and it is NOT read as too long here; the host checks Codex's cap before sending instead
//!   (`crates/host/src/codex_input.rs`).
//! - claude-agent-acp without a session failure, from source: error -32603 with
//!   `data.errorKind`. The SDK's `terminal_reason: "prompt_too_long"` is read too, should the
//!   adapter forward it.

use crate::TooLong;
use crate::error::RpcData;

/// Claude Code's text for a prompt over the model's context, recorded as the `sessionFailure`
/// title. Its categories (`service`, `request`) also hold other failures, so only this title says
/// "too long"; matched as a prefix, ASCII case ignored, since the API may append token counts.
const CLAUDE_PROMPT_TOO_LONG: &str = "prompt is too long";

/// The structured values that say too long: the SDK's terminal reason, and Codex's error kind
/// for a full context should codex-acp forward it as an error.
const TOO_LONG_KINDS: &[&str] = &[
    "prompt_too_long",
    "context_window_exceeded",
    "contextWindowExceeded",
];

/// How an AIR `sessionFailure` with these fields says the turn was too long to read, if it does.
/// `ContextExhausted` ends a turn `TooLong` only for codex-acp; the host decides by the agent.
pub(crate) fn session_failure_says_too_long(
    category: Option<&str>,
    severity: Option<&str>,
    actions: Option<&[&str]>,
    title: Option<&str>,
) -> Option<TooLong> {
    match (category, severity, actions) {
        (Some("limit"), Some("error"), Some(["new_session"])) => Some(TooLong::ContextExhausted),
        (Some("service" | "request"), Some("error"), _)
            if title.is_some_and(starts_with_claude_text) =>
        {
            Some(TooLong::PromptTooLong)
        }
        _ => None,
    }
}

/// Whether a JSON-RPC error's allowlisted `data` fields say the question was too long to read.
/// No free text is read: Codex's own refusal text ("Input exceeds the maximum length of ...")
/// never reaches the wire in codex-acp 2.2.2 (recorded), so its prefix match was removed.
pub(crate) fn rpc_says_too_long(data: &RpcData) -> bool {
    [
        data.error_kind(),
        data.terminal_reason(),
        data.codex_error_info(),
    ]
    .into_iter()
    .flatten()
    .any(|kind| TOO_LONG_KINDS.contains(&kind))
}

fn starts_with_claude_text(title: &str) -> bool {
    title
        .get(..CLAUDE_PROMPT_TOO_LONG.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(CLAUDE_PROMPT_TOO_LONG))
}

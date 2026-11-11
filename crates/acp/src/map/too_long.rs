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
//!   the same shape and reads the same.
//! - codex-acp 2.2.2, a question over Codex's 1,048,576 characters, recorded
//!   (`codex-overcap-reply.jsonl`): `end_turn` with a `sessionFailure` of category `service`,
//!   title "Codex encountered an internal error.", actions `["retry", "new_session"]`. The length
//!   reason is only on codex-acp's stderr, so the wire cannot tell it from a real internal error,
//!   and it is NOT read as too long here.
//! - claude-agent-acp without a session failure, from source: error -32603 with
//!   `data.errorKind`. The SDK's `terminal_reason: "prompt_too_long"` is read too, should the
//!   adapter forward it.

use crate::error::RpcData;

/// Claude Code's text for a prompt over the model's context, recorded as the `sessionFailure`
/// title. Its categories (`service`, `request`) also hold other failures, so only this title says
/// "too long"; matched as a prefix, ASCII case ignored, since the API may append token counts.
const CLAUDE_PROMPT_TOO_LONG: &str = "prompt is too long";

/// Codex's refusal of input over its 1,048,576 characters (`MAX_USER_INPUT_TEXT_CHARS`), matched
/// by its exact prefix and nothing looser. The recording showed it never reaches the wire in
/// codex-acp 2.2.2 (it is only on stderr; the wire says "Codex encountered an internal error."),
/// so this match is kept only for an adapter that forwards Codex's text as an RPC error.
const CODEX_INPUT_TOO_LONG: &str = "Input exceeds the maximum length of";

/// The structured values that say too long: the SDK's terminal reason, and Codex's error kind
/// for a full context should codex-acp forward it as an error.
const TOO_LONG_KINDS: &[&str] = &[
    "prompt_too_long",
    "context_window_exceeded",
    "contextWindowExceeded",
];

/// Whether an AIR `sessionFailure` with these fields says the turn was too long to read.
pub(crate) fn session_failure_says_too_long(
    category: Option<&str>,
    severity: Option<&str>,
    actions: Option<&[&str]>,
    title: Option<&str>,
) -> bool {
    match (category, severity, actions) {
        (Some("limit"), Some("error"), Some(["new_session"])) => true,
        (Some("service" | "request"), Some("error"), _) => {
            title.is_some_and(starts_with_claude_text)
        }
        _ => false,
    }
}

/// Whether a JSON-RPC error to `session/prompt` says the question was too long to read: a
/// structured value first (`data`'s allowlisted fields), else Codex's exact refusal text in the
/// message or at `data.message`.
pub(crate) fn rpc_says_too_long(message: &str, data: &RpcData, data_text: Option<&str>) -> bool {
    let structured = [
        data.error_kind(),
        data.terminal_reason(),
        data.codex_error_info(),
    ]
    .into_iter()
    .flatten()
    .any(|kind| TOO_LONG_KINDS.contains(&kind));
    structured
        || message.starts_with(CODEX_INPUT_TOO_LONG)
        || data_text.is_some_and(|text| text.starts_with(CODEX_INPUT_TOO_LONG))
}

fn starts_with_claude_text(title: &str) -> bool {
    title
        .get(..CLAUDE_PROMPT_TOO_LONG.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(CLAUDE_PROMPT_TOO_LONG))
}

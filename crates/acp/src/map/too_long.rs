//! The model refused a turn as too long to read: the question by itself, or the conversation
//! with it. Every shape that says so is read here and nowhere else, so a live recording changes
//! only this file and its scenario. None of it is a plan limit: it never becomes a
//! [`LimitSignal`](crate::LimitSignal), so it never reaches the limits store nor the Codex
//! limits read.
//!
//! The shapes, until the live recordings replace them (each from the adapter's source):
//! - codex-acp 2.2.2, a full context: Codex's `contextWindowExceeded` becomes
//!   `context_exhausted`, sent to an AIR client as `end_turn` with a `sessionFailure` of
//!   category `limit`, severity `error`, actions `["new_session"]`.
//! - codex-acp, a question over Codex's 1,048,576 characters: Codex refuses the input with
//!   "Input exceeds the maximum length of 1048576 characters."; how codex-acp passes it on is not
//!   yet recorded, most likely as a JSON-RPC error to `session/prompt` with Codex's text at
//!   `data.message` (as for its other errors).
//! - claude-agent-acp 0.89.1, an AIR client: a result with `is_error` becomes `end_turn` with a
//!   `sessionFailure` from the assistant's error kind; an API `invalid_request` is category
//!   `request`, severity `error`, no actions, titled with the error's text, which Claude Code
//!   writes "Prompt is too long". Its `context_exhausted` (category `limit`, actions
//!   `["new_session"]`) has Codex's shape and reads the same.
//! - claude-agent-acp without a session failure: error -32603 with `data.errorKind`. The SDK's
//!   `terminal_reason: "prompt_too_long"` is read too, should the adapter forward it.

use crate::error::RpcData;

/// Claude Code's text for an API refusal of a prompt over the model's context. The `request`
/// category also holds other bad requests, so only this title says "too long"; matched as a
/// prefix, ASCII case ignored, since the API appends the token counts.
const CLAUDE_PROMPT_TOO_LONG: &str = "prompt is too long";

/// Codex's refusal of input over its 1,048,576 characters (`MAX_USER_INPUT_TEXT_CHARS`). It
/// comes with no structured field Clauseo has seen, so its exact prefix is matched, and nothing
/// looser: any other error text stays the error it was.
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
        (Some("request"), Some("error"), _) => title.is_some_and(starts_with_claude_text),
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

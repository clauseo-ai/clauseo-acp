//! A turn the agent gave up on: its answer carried an AIR `sessionFailure` of severity `error`
//! with `retry` or `new_session`, in a category no other rule reads (`clauseo_acp::AgentFailed`),
//! as codex-acp 2.2.2 ends a turn on Codex's internal error. The turn ends `Failed` with
//! `agent.service_error`, so the lawyer sees a failure line and Try again (no Restart), and the
//! queue holds. A JSON-RPC error to `session/prompt` keeps `agent.protocol_error`: it cannot be
//! told from a malformed reply. The agent's title is never shown nor logged:
//! the detail and the log name only its allowlisted category.

use clauseo_acp::PromptEnd;
use clauseo_protocol::{ErrorCode, ThreadId};

use super::warning;

/// The code and detail `thread`'s prompt that ended `result` fails with when the agent gave up
/// on it; none for a turn that ends `TooLong` (`too_long`), which wins.
pub(in crate::actor::turn) fn failed(
    thread: ThreadId,
    result: &Result<PromptEnd, clauseo_acp::Error>,
    too_long: bool,
) -> Option<(ErrorCode, Option<String>)> {
    let failed = result
        .as_ref()
        .ok()?
        .failed
        .as_ref()
        .filter(|_| !too_long)?;
    let category = warning::category(failed.category.as_deref());
    tracing::info!(%thread, category, "the agent gave up on the turn");
    let detail = format!("the agent gave up on the turn (sessionFailure {category})");
    Some((ErrorCode::AgentServiceError, Some(detail)))
}

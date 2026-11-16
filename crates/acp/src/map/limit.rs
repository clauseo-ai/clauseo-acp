//! Plan limit signals and lost connections: Claude's `_claude/rateLimit` and the AIR
//! `sessionFailure`. `_meta` is read at these two keys only, and only on `usage_update`,
//! `session_info_update` and the prompt response. A `sessionFailure` that says the turn was
//! too long is not a limit (`too_long.rs`).

use clauseo_protocol::{Finite, LimitStopReason, Timestamp};
use serde_json::{Map, Value};

use super::too_long::session_failure_says_too_long;
use crate::event::{AgentFailed, ConnectionLost, LimitSignal, LimitStatus, ProtocolWarning};

/// `_meta["_claude/rateLimit"]` of a `usage_update`, when present: the signal, or a warning for
/// a `status` this crate does not know. A missing or unreadable field is `None`.
pub(crate) fn rate_limit(
    meta: &Map<String, Value>,
) -> Option<Result<LimitSignal, ProtocolWarning>> {
    let limit = meta.get("_claude/rateLimit")?.as_object()?;
    let status = match limit.get("status").and_then(Value::as_str) {
        Some("allowed") => LimitStatus::Allowed,
        Some("allowed_warning") => LimitStatus::Warning,
        Some("rejected") => LimitStatus::Rejected,
        other => {
            let status = other.map(str::to_owned);
            return Some(Err(ProtocolWarning::UnknownLimitStatus { status }));
        }
    };
    let used_percent = limit
        .get("utilization")
        .and_then(Value::as_f64)
        .and_then(|utilization| Finite::new(utilization * 100.0));
    let resets_at = limit
        .get("resetsAt")
        .and_then(Value::as_i64)
        .and_then(|seconds| seconds.checked_mul(1000))
        .and_then(|millis| Timestamp::new(millis).ok());
    Some(Ok(LimitSignal::RateLimit {
        window: limit
            .get("rateLimitType")
            .and_then(Value::as_str)
            .map(str::to_owned),
        status,
        used_percent,
        resets_at,
    }))
}

/// What an AIR `sessionFailure` says.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Failure {
    /// A plan limit stopped the turn.
    Limit(LimitSignal),
    /// The model refused the turn as too long to read (`too_long.rs`). Not a plan limit.
    TooLong(crate::TooLong),
    /// The agent could not reach its service.
    Connection(ConnectionLost),
    /// The agent gave up on the turn (`AgentFailed`).
    Failed(AgentFailed),
    /// Any other failure, which the driver only reports.
    Warning(ProtocolWarning),
}

/// `_meta.jetbrains.air.sessionFailure`, when present. Both adapters set `category`, `severity`
/// and `actions` from one policy table: `limit`, `error` and no actions is the plan window
/// (`quota_exhausted`); with `["retry"]` it is a temporary limit (`rate_limited`). A failure
/// that says the turn was too long to read is [`Failure::TooLong`], read by
/// [`session_failure_says_too_long`]. Category `connection` is codex-acp 2.2.2's
/// `transport_lost` (Codex's `httpConnectionFailed`, `responseStreamConnectionFailed`,
/// `responseStreamDisconnected`, `responseTooManyFailedAttempts`): severity `warning` with no
/// actions while Codex retries, `error` with `["retry", "new_session"]` once it gave up. The
/// order is limit, too long, connection, failed, warning; too long reads only categories `limit`,
/// `service` and `request`, so no failure is both too long and a lost connection. Severity
/// `error` with `retry` or `new_session` in its actions, in any other category but `limit`, is
/// [`Failure::Failed`]: the agent gave up on the turn. Any other failure is a warning.
pub(crate) fn session_failure(meta: &Map<String, Value>) -> Option<Failure> {
    let failure = meta
        .get("jetbrains")?
        .get("air")?
        .get("sessionFailure")?
        .as_object()?;
    let text = |key: &str| failure.get(key).and_then(Value::as_str).map(str::to_owned);
    let (category, severity, title) = (text("category"), text("severity"), text("title"));
    let actions: Option<Vec<&str>> = failure
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| actions.iter().filter_map(Value::as_str).collect());
    let reason = match (category.as_deref(), severity.as_deref(), actions.as_deref()) {
        (Some("limit"), Some("error"), Some([])) => Some(LimitStopReason::PlanWindow),
        (Some("limit"), Some("error"), Some(["retry"])) => Some(LimitStopReason::Temporary),
        _ => None,
    };
    if let Some(reason) = reason {
        return Some(Failure::Limit(LimitSignal::Stopped { reason, title }));
    }
    if let Some(kind) = session_failure_says_too_long(
        category.as_deref(),
        severity.as_deref(),
        actions.as_deref(),
        title.as_deref(),
    ) {
        return Some(Failure::TooLong(kind));
    }
    if category.as_deref() == Some("connection") {
        let gave_up = severity.as_deref() == Some("error");
        return Some(Failure::Connection(ConnectionLost { gave_up, title }));
    }
    let gives_up = actions.as_deref().is_some_and(|actions| {
        actions
            .iter()
            .any(|a| matches!(*a, "retry" | "new_session"))
    });
    if severity.as_deref() == Some("error") && category.as_deref() != Some("limit") && gives_up {
        return Some(Failure::Failed(AgentFailed { category, title }));
    }
    Some(Failure::Warning(ProtocolWarning::SessionFailure {
        category,
        severity,
        title,
    }))
}

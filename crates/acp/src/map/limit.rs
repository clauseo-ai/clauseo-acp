//! Plan limit signals: Claude's `_claude/rateLimit` and the AIR `sessionFailure`. `_meta` is
//! read at these two keys only, and only on `usage_update`, `session_info_update` and the
//! prompt response.

use clauseo_protocol::{Finite, LimitStopReason, Timestamp};
use serde_json::{Map, Value};

use crate::event::{LimitSignal, LimitStatus, ProtocolWarning};

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
            let status = other.map_or_else(|| "(missing)".to_owned(), str::to_owned);
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

/// `_meta.jetbrains.air.sessionFailure`, when present. Both adapters set `category`, `severity`
/// and `actions` from one policy table: `limit`, `error` and no actions is the plan window
/// (`quota_exhausted`); with `["retry"]` it is a temporary limit (`rate_limited`). Any other
/// failure is a warning.
pub(crate) fn session_failure(
    meta: &Map<String, Value>,
) -> Option<Result<LimitSignal, ProtocolWarning>> {
    let failure = meta
        .get("jetbrains")?
        .get("air")?
        .get("sessionFailure")?
        .as_object()?;
    let text = |key: &str| {
        failure
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let (category, severity, title) = (text("category"), text("severity"), text("title"));
    let actions: Option<Vec<&str>> = failure
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| actions.iter().filter_map(Value::as_str).collect());
    let reason = match (category.as_str(), severity.as_str(), actions.as_deref()) {
        ("limit", "error", Some([])) => Some(LimitStopReason::PlanWindow),
        ("limit", "error", Some(["retry"])) => Some(LimitStopReason::Temporary),
        _ => None,
    };
    Some(match reason {
        Some(reason) => Ok(LimitSignal::Stopped { reason, title }),
        None => Err(ProtocolWarning::SessionFailure {
            category,
            severity,
            title,
        }),
    })
}

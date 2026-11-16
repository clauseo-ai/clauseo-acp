//! An ACP warning, for the log: its kind, and for a `sessionFailure` the host does not act on
//! its category and severity, never its title (the agent's text).

use clauseo_acp::ProtocolWarning as W;
use clauseo_protocol::{AgentSessionId, ThreadId};

/// The `sessionFailure` categories the log takes as they are: codex-acp 2.2.2's
/// (`SESSION_FAILURE_POLICY`, and `unknown` for its notices).
const CATEGORIES: [&str; 7] = [
    "limit",
    "context",
    "access",
    "service",
    "request",
    "connection",
    "unknown",
];

/// The `sessionFailure` severities the log takes as they are.
const SEVERITIES: [&str; 3] = ["error", "warning", "info"];

/// The log target both lines keep: the module they were logged from before this file split,
/// so a reader's filter on it still finds them.
const TARGET: &str = "clauseo_host::actor::turn::updates";

/// Logs `warning` from `thread`'s agent about `session`.
pub(super) fn log(thread: ThreadId, session: Option<&AgentSessionId>, warning: &W) {
    let kind = kind(warning);
    if let W::SessionFailure {
        category, severity, ..
    } = warning
    {
        let category = word(&CATEGORIES, category.as_deref());
        let severity = word(&SEVERITIES, severity.as_deref());
        tracing::warn!(
            target: TARGET,
            %thread,
            ?session,
            warning = kind,
            category,
            severity,
            "the agent reported a failure the host does not act on"
        );
    } else {
        tracing::warn!(
            target: TARGET,
            %thread,
            ?session,
            warning = kind,
            "the agent sent something unreadable"
        );
    }
}

/// A `sessionFailure`'s category, for the log: one of [`CATEGORIES`], `none` or `other`.
pub(super) fn category(value: Option<&str>) -> &'static str {
    word(&CATEGORIES, value)
}

/// A warning's kind, for the log: its fields hold what the agent sent.
const fn kind(warning: &W) -> &'static str {
    match warning {
        W::MalformedLine { .. } => "malformed_line",
        W::UnknownUpdate { .. } => "unknown_update",
        W::InvalidUpdate { .. } => "invalid_update",
        W::UnknownLimitStatus { .. } => "unknown_limit_status",
        W::SessionFailure { .. } => "session_failure",
    }
}

/// A protocol word the agent sent (a `category`, a `severity`) for the log: as it is when it
/// is one of `known`; else `other`, or `none` when it is missing.
fn word(known: &[&'static str], value: Option<&str>) -> &'static str {
    match value {
        None => "none",
        Some(value) => known
            .iter()
            .copied()
            .find(|known| *known == value)
            .unwrap_or("other"),
    }
}

#[cfg(test)]
mod tests {
    use super::{CATEGORIES, SEVERITIES, word};

    #[test]
    fn only_known_protocol_words_reach_the_log() {
        assert_eq!(word(&CATEGORIES, Some("connection")), "connection");
        assert_eq!(word(&CATEGORIES, Some("limit")), "limit");
        assert_eq!(word(&SEVERITIES, Some("warning")), "warning");
        assert_eq!(word(&CATEGORIES, None), "none");
        assert_eq!(word(&CATEGORIES, Some("")), "other");
        assert_eq!(word(&CATEGORIES, Some("budget_exhausted")), "other");
        assert_eq!(word(&SEVERITIES, Some("connection")), "other");
        assert_eq!(
            word(&CATEGORIES, Some("Connection refused (os error 61)")),
            "other"
        );
    }
}

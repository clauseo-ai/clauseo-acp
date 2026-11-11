//! An ACP warning, for the log: its kind, and for a `sessionFailure` the host does not act on
//! its category and severity, never its title (the agent's text).

use clauseo_acp::ProtocolWarning as W;
use clauseo_protocol::{AgentSessionId, ThreadId};

/// The longest category or severity the log takes as it is.
const WORD_MAX: usize = 32;

/// Logs `warning` from `thread`'s agent about `session`.
pub(super) fn log(thread: ThreadId, session: Option<&AgentSessionId>, warning: &W) {
    let kind = kind(warning);
    if let W::SessionFailure {
        category, severity, ..
    } = warning
    {
        let (category, severity) = (word(category.as_deref()), word(severity.as_deref()));
        tracing::warn!(
            %thread,
            ?session,
            warning = kind,
            category,
            severity,
            "the agent reported a failure the host does not act on"
        );
    } else {
        tracing::warn!(%thread, ?session, warning = kind, "the agent sent something unreadable");
    }
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
/// is lowercase ASCII letters, digits and `_`, up to [`WORD_MAX`] bytes; else `other`, or
/// `none` when it is missing.
fn word(value: Option<&str>) -> &str {
    match value {
        None => "none",
        Some(value)
            if !value.is_empty()
                && value.len() <= WORD_MAX
                && value
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') =>
        {
            value
        }
        Some(_) => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::word;

    #[test]
    fn only_protocol_words_reach_the_log() {
        assert_eq!(word(Some("connection")), "connection");
        assert_eq!(word(Some("budget_exhausted")), "budget_exhausted");
        assert_eq!(word(None), "none");
        assert_eq!(word(Some("")), "other");
        assert_eq!(word(Some("Connection refused (os error 61)")), "other");
        assert_eq!(word(Some(&"a".repeat(33))), "other");
    }
}

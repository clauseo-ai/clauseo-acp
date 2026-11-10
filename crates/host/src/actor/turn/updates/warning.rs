//! An ACP warning's kind, for the log.

/// A warning's kind, for the log: its fields hold what the agent sent.
pub(super) const fn kind(warning: &clauseo_acp::ProtocolWarning) -> &'static str {
    use clauseo_acp::ProtocolWarning as W;
    match warning {
        W::MalformedLine { .. } => "malformed_line",
        W::UnknownUpdate { .. } => "unknown_update",
        W::InvalidUpdate { .. } => "invalid_update",
        W::UnknownLimitStatus { .. } => "unknown_limit_status",
        W::SessionFailure { .. } => "session_failure",
    }
}

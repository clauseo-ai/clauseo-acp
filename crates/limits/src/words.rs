//! The words the crate chooses: window labels and plan names, plain English made from the
//! agents' ids. The new design's words replace these later.

use clauseo_protocol::WindowKind;

/// Claude's session window id, in the usage read and in `_claude/rateLimit`.
pub(crate) const CLAUDE_FIVE_HOUR: &str = "five_hour";
/// Claude's weekly window id, in the usage read and in `_claude/rateLimit`.
pub(crate) const CLAUDE_SEVEN_DAY: &str = "seven_day";

/// The kind and label of a Claude window that a live signal may add before any read named it:
/// only the 5-hour and the weekly window. Any other id (`overage`, a model's bucket) waits for a
/// read to name it.
pub(crate) fn claude_live_window(id: &str) -> Option<(WindowKind, &'static str)> {
    match id {
        CLAUDE_FIVE_HOUR => Some((WindowKind::Session, "5-hour limit")),
        CLAUDE_SEVEN_DAY => Some((WindowKind::Weekly, "Weekly limit")),
        _ => None,
    }
}

/// One fixed window of Claude's usage read: its key in `rate_limits`, which is also its id, its
/// kind, its label and its length in minutes.
pub(crate) struct ClaudeWindow {
    /// The key in `rate_limits` and the window's id.
    pub(crate) key: &'static str,
    /// What it counts.
    pub(crate) kind: WindowKind,
    /// Its label.
    pub(crate) label: &'static str,
    /// Its length in minutes.
    pub(crate) duration_mins: u32,
}

/// A week, in minutes.
const WEEK_MINS: u32 = 7 * 24 * 60;

/// The fixed windows of Claude's usage read, in the order the read lists them.
pub(crate) const CLAUDE_WINDOWS: [ClaudeWindow; 5] = [
    ClaudeWindow {
        key: CLAUDE_FIVE_HOUR,
        kind: WindowKind::Session,
        label: "5-hour limit",
        duration_mins: 5 * 60,
    },
    ClaudeWindow {
        key: CLAUDE_SEVEN_DAY,
        kind: WindowKind::Weekly,
        label: "Weekly limit",
        duration_mins: WEEK_MINS,
    },
    ClaudeWindow {
        key: "seven_day_opus",
        kind: WindowKind::Model,
        label: "Opus weekly limit",
        duration_mins: WEEK_MINS,
    },
    ClaudeWindow {
        key: "seven_day_sonnet",
        kind: WindowKind::Model,
        label: "Sonnet weekly limit",
        duration_mins: WEEK_MINS,
    },
    ClaudeWindow {
        key: "seven_day_oauth_apps",
        kind: WindowKind::Other,
        label: "Weekly limit for connected apps",
        duration_mins: WEEK_MINS,
    },
];

/// The id of the one per-model weekly window when `model_scoped` lists exactly one: the live
/// `rateLimitType` of that name lands on it.
pub(crate) const CLAUDE_OVERAGE_INCLUDED: &str = "seven_day_overage_included";
/// The id prefix of each per-model weekly window when `model_scoped` lists two or more; the
/// array index follows it.
pub(crate) const CLAUDE_MODEL_SCOPED: &str = "model_scoped:";
/// The length of a per-model weekly window, in minutes.
pub(crate) const CLAUDE_MODEL_SCOPED_MINS: u32 = WEEK_MINS;

/// The label of a per-model weekly window, from the server's name for the model.
pub(crate) fn claude_model_label(display_name: &str) -> String {
    format!("{display_name} weekly limit")
}

/// The plan's name for Claude's `subscription_type`; `None` for a type this crate does not know.
pub(crate) fn claude_plan(subscription_type: &str) -> Option<&'static str> {
    match subscription_type {
        "pro" => Some("Claude Pro"),
        "max" => Some("Claude Max"),
        "team" => Some("Claude Team"),
        "enterprise" => Some("Claude Enterprise"),
        _ => None,
    }
}

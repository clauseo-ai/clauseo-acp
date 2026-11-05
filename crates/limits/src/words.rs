//! The words the crate chooses: window labels and plan names, plain English made from the
//! agents' ids. The new design's words replace these later.

use clauseo_protocol::WindowKind;

/// Claude's session window id, in the usage read and in `_claude/rateLimit`.
pub(crate) const CLAUDE_FIVE_HOUR: &str = "five_hour";
/// Claude's weekly window id, in the usage read and in `_claude/rateLimit`.
pub(crate) const CLAUDE_SEVEN_DAY: &str = "seven_day";

/// The kind and label of a Claude window that a live signal may add before any read named it:
/// only the 5-hour and the weekly window, as [`CLAUDE_WINDOWS`] names them for a read. Any
/// other id (`overage`, a model's bucket) waits for a read to name it.
pub(crate) fn claude_live_window(id: &str) -> Option<(WindowKind, &'static str)> {
    if id != CLAUDE_FIVE_HOUR && id != CLAUDE_SEVEN_DAY {
        return None;
    }
    CLAUDE_WINDOWS
        .iter()
        .find(|window| window.key == id)
        .map(|window| (window.kind, window.label))
}

/// One fixed window of Claude's usage read: its key in `rate_limits`, which is also its id, its
/// kind, its label, its length in minutes and, for a per-model window, its model.
pub(crate) struct ClaudeWindow {
    /// The key in `rate_limits` and the window's id.
    pub(crate) key: &'static str,
    /// What it counts.
    pub(crate) kind: WindowKind,
    /// Its label.
    pub(crate) label: &'static str,
    /// Its length in minutes.
    pub(crate) duration_mins: u32,
    /// The model a per-model window counts, as `model_scoped` would name it: a `model_scoped`
    /// window of that name replaces this one.
    pub(crate) model: Option<&'static str>,
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
        model: None,
    },
    ClaudeWindow {
        key: CLAUDE_SEVEN_DAY,
        kind: WindowKind::Weekly,
        label: "Weekly limit",
        duration_mins: WEEK_MINS,
        model: None,
    },
    ClaudeWindow {
        key: "seven_day_opus",
        kind: WindowKind::Model,
        label: "Opus weekly limit",
        duration_mins: WEEK_MINS,
        model: Some("Opus"),
    },
    ClaudeWindow {
        key: "seven_day_sonnet",
        kind: WindowKind::Model,
        label: "Sonnet weekly limit",
        duration_mins: WEEK_MINS,
        model: Some("Sonnet"),
    },
    ClaudeWindow {
        key: "seven_day_oauth_apps",
        kind: WindowKind::Other,
        label: "Weekly limit for connected apps",
        duration_mins: WEEK_MINS,
        model: None,
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
/// Public so desktop's sign-in line names the plan as `AccountLimits.plan` does (decision
/// record 0048).
pub fn claude_plan(subscription_type: &str) -> Option<&'static str> {
    match subscription_type {
        "pro" => Some("Claude Pro"),
        "max" => Some("Claude Max"),
        "team" => Some("Claude Team"),
        "enterprise" => Some("Claude Enterprise"),
        _ => None,
    }
}

/// The plan's name for Codex's `planType` wire value; `None` for one whose plain name is not
/// clear from its id (`self_serve_business_*`, `ent26`, `enterprise_cbp_*`, `unknown`, and any
/// value this crate does not know). Public, as [`claude_plan`] is.
pub fn codex_plan(plan_type: &str) -> Option<&'static str> {
    match plan_type {
        "free" => Some("ChatGPT Free"),
        "go" => Some("ChatGPT Go"),
        "plus" => Some("ChatGPT Plus"),
        "pro" => Some("ChatGPT Pro"),
        "prolite" => Some("ChatGPT Pro Lite"),
        "promax" => Some("ChatGPT Pro Max"),
        "team" => Some("ChatGPT Team"),
        "business" => Some("ChatGPT Business"),
        "enterprise" => Some("ChatGPT Enterprise"),
        "edu" => Some("ChatGPT Edu"),
        "edu_plus" => Some("ChatGPT Edu Plus"),
        "edu_pro" => Some("ChatGPT Edu Pro"),
        _ => None,
    }
}

/// The label of Codex's 5-hour window.
pub(crate) const CODEX_SESSION: &str = "5-hour limit";
/// The label of Codex's weekly window.
pub(crate) const CODEX_WEEKLY: &str = "Weekly limit";
/// The label of a Codex window of 28 to 31 days, and of Free's and Go's window of unknown length.
pub(crate) const CODEX_MONTHLY: &str = "Monthly limit";
/// The label of a Codex window of unknown length on any other plan.
pub(crate) const CODEX_USAGE: &str = "Usage limit";

/// The label of a Codex window of another length, in minutes (positive): "N-hour limit" for
/// whole hours under 48, "N-day limit" for whole days, "N-hour limit" for other whole hours,
/// else "N-minute limit".
pub(crate) fn codex_window_label(mins: i64) -> String {
    let (hours, days) = (mins / 60, mins / (24 * 60));
    if mins % 60 == 0 && hours < 48 {
        format!("{hours}-hour limit")
    } else if mins % (24 * 60) == 0 {
        format!("{days}-day limit")
    } else if mins % 60 == 0 {
        format!("{hours}-hour limit")
    } else {
        format!("{mins}-minute limit")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_row_and_a_read_row_get_the_same_kind_and_label() {
        for id in [CLAUDE_FIVE_HOUR, CLAUDE_SEVEN_DAY] {
            let read = CLAUDE_WINDOWS
                .iter()
                .find(|window| window.key == id)
                .map(|window| (window.kind, window.label));
            assert_eq!(claude_live_window(id), read, "{id}");
            assert!(read.is_some(), "{id}");
        }
        assert_eq!(
            claude_live_window(CLAUDE_FIVE_HOUR),
            Some((WindowKind::Session, "5-hour limit"))
        );
        assert_eq!(
            claude_live_window(CLAUDE_SEVEN_DAY),
            Some((WindowKind::Weekly, "Weekly limit"))
        );
        assert_eq!(claude_live_window("seven_day_opus"), None);
        assert_eq!(claude_live_window("overage"), None);
    }
}

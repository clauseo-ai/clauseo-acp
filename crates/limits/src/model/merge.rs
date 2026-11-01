//! The merge (research 30 §1.1, §1.3): a good read replaces the account's data wholesale; a
//! failed read keeps the last good numbers as stale; a live signal upserts one window.

use clauseo_protocol::{
    AccountLimits, AgentKind, LimitSource, LimitStopReason, LimitWindow, StaleReason, Timestamp,
    UnavailableReason,
};

use crate::model::{AgentState, LiveSignal, LiveStatus, Row, kind_rank};
use crate::policy::Action;
use crate::readers::{ReadFailure, ReadOutcome};
use crate::words::claude_live_window;

/// Merges the outcome of a read of `state`'s agent, at `now`.
pub(crate) fn apply_read(
    state: &mut AgentState,
    outcome: ReadOutcome,
    now: Timestamp,
) -> Option<Action> {
    match outcome {
        ReadOutcome::Usage { limits, reached } => {
            if limits.agent == state.agent {
                Some(good_read(state, limits, reached, now))
            } else {
                tracing::warn!(
                    agent = ?state.agent,
                    read = ?limits.agent,
                    "a limits read answered for another agent; its figures are dropped"
                );
                failed_read(state).then(|| notice(state))
            }
        }
        ReadOutcome::MethodMissing => {
            state.method_missing = true;
            failed_read(state);
            Some(notice(state))
        }
        ReadOutcome::Failed(failure) => {
            log_failure(state.agent, &failure);
            failed_read(state).then(|| notice(state))
        }
    }
}

/// Replaces the account's data with `limits`. Live updates that arrived while the read ran are
/// dropped with the rest: the read is the newer whole.
fn good_read(
    state: &mut AgentState,
    limits: AccountLimits,
    reached: Option<LimitStopReason>,
    now: Timestamp,
) -> Action {
    let source = match limits.source {
        source @ (LimitSource::NotApplicable
        | LimitSource::Unavailable {
            reason: UnavailableReason::MissingScope,
        }) => source,
        _ => LimitSource::Fresh { checked_at: now },
    };
    let mut windows = limits.windows;
    windows.sort_by_key(|window| kind_rank(window.kind));
    let held = AccountLimits {
        source,
        windows,
        ..limits
    };
    state.account = Some(held.account.clone());
    state.plan.clone_from(&held.plan);
    state.billing = held.billing;
    state.rows = held
        .windows
        .iter()
        .map(|window| Row::new(window.clone(), now))
        .collect();
    state.at_limit = held.at_limit;
    state.source = held.source.clone();
    state.method_missing = false;
    state.reached = reached;
    Action::Changed(held)
}

/// Marks a failed read: the last good numbers stay as `Stale { ReadFailed }`, or the source is
/// `Unavailable { ReadFailed }` when there are none. `NotApplicable` stays. Rows keep their
/// flags, so a row past its reset shows no old percentage. Returns whether the source changed.
fn failed_read(state: &mut AgentState) -> bool {
    let source = match state.source {
        LimitSource::NotApplicable => return false,
        LimitSource::Fresh { checked_at }
        | LimitSource::Stale { checked_at, .. }
        | LimitSource::LiveOnly {
            updated_at: checked_at,
        } => LimitSource::Stale {
            checked_at,
            reason: StaleReason::ReadFailed,
        },
        LimitSource::Loading | LimitSource::Unavailable { .. } | LimitSource::Unknown { .. } => {
            LimitSource::Unavailable {
                reason: UnavailableReason::ReadFailed,
            }
        }
    };
    let changed = source != state.source;
    state.source = source;
    changed
}

/// Logs a failed read at `warn` with its kind and exit code only; any text it carries (the
/// redacted stderr tail, a launch error, a parse error) goes to `debug`.
fn log_failure(agent: AgentKind, failure: &ReadFailure) {
    match failure {
        ReadFailure::TimedOut => {
            tracing::warn!(?agent, kind = "timed_out", "limits read failed");
        }
        ReadFailure::LaunchFailed(detail) => {
            tracing::warn!(?agent, kind = "launch_failed", "limits read failed");
            tracing::debug!(?agent, %detail, "limits reader did not start");
        }
        ReadFailure::Exited { code, stderr_tail } => {
            tracing::warn!(?agent, kind = "exited", ?code, "limits read failed");
            tracing::debug!(?agent, %stderr_tail, "limits reader's stderr");
        }
        ReadFailure::Malformed(detail) => {
            tracing::warn!(?agent, kind = "malformed", "limits read failed");
            tracing::debug!(?agent, %detail, "limits reader's answer did not read");
        }
    }
}

/// Merges a live signal at `now`. A signal that changes nothing returns `None` and touches
/// nothing.
pub(crate) fn apply_live(
    state: &mut AgentState,
    signal: &LiveSignal,
    now: Timestamp,
) -> Option<Action> {
    if state.source == LimitSource::NotApplicable {
        return None;
    }
    let Some(id) = signal.window.clone() else {
        tracing::debug!(agent = ?state.agent, "a live limit signal without a window is ignored");
        return None;
    };
    if state.account.is_none() && !state.method_missing {
        return None;
    }
    if let Some(row) = state.rows.iter_mut().find(|row| row.window.id == id) {
        if !upsert(row, signal) {
            return None;
        }
        row.updated_at = now;
        row.stale_noted = false;
        row.reset_passed = false;
    } else {
        let row = live_row(state.agent, id, signal, now)?;
        insert_sorted(&mut state.rows, row);
    }
    state.source = match state.source {
        _ if state.method_missing => LimitSource::LiveOnly { updated_at: now },
        LimitSource::Loading | LimitSource::Unavailable { .. } | LimitSource::LiveOnly { .. } => {
            LimitSource::LiveOnly { updated_at: now }
        }
        ref source => source.clone(),
    };
    Some(notice(state))
}

/// Applies `signal` to `row`'s window; returns whether anything changed. A rejection is a
/// flag, never a figure: the percentage stays as the agent sent it.
fn upsert(row: &mut Row, signal: &LiveSignal) -> bool {
    let used_percent = signal.used_percent.unwrap_or(row.window.used_percent);
    let resets_at = signal.resets_at.or(row.window.resets_at);
    let warning = match signal.status {
        LiveStatus::Allowed => false,
        LiveStatus::Warning => true,
        LiveStatus::Rejected => row.window.warning,
    };
    let rejected = row.rejected || signal.status == LiveStatus::Rejected;
    let changed = used_percent != row.window.used_percent
        || resets_at != row.window.resets_at
        || warning != row.window.warning
        || rejected != row.rejected;
    row.window.used_percent = used_percent;
    row.window.resets_at = resets_at;
    row.window.warning = warning;
    row.rejected = rejected;
    changed
}

/// A row for a window no read named yet: only Claude's 5-hour and weekly windows, and only with
/// a figure. Anything else (an `overage` bucket) waits for a read to name it.
fn live_row(agent: AgentKind, id: String, signal: &LiveSignal, now: Timestamp) -> Option<Row> {
    if agent != AgentKind::Claude {
        return None;
    }
    let (kind, label) = claude_live_window(&id)?;
    let used_percent = signal.used_percent?;
    let window = LimitWindow {
        id,
        kind,
        label: label.to_owned(),
        used_percent,
        resets_at: signal.resets_at,
        duration_mins: None,
        warning: signal.status == LiveStatus::Warning,
    };
    Some(Row {
        rejected: signal.status == LiveStatus::Rejected,
        ..Row::new(window, now)
    })
}

/// Inserts `row` after every row of its kind or an earlier one.
fn insert_sorted(rows: &mut Vec<Row>, row: Row) {
    let rank = kind_rank(row.window.kind);
    let at = rows
        .iter()
        .position(|other| kind_rank(other.window.kind) > rank)
        .unwrap_or(rows.len());
    rows.insert(at, row);
}

/// What to publish after a change: the limits once the account is known, else the view only.
fn notice(state: &AgentState) -> Action {
    state.limits().map_or(Action::ViewChanged, Action::Changed)
}

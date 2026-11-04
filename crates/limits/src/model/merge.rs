//! The merge (research 30 §1.1, §1.3): a good read replaces the account's data wholesale; a
//! failed read keeps the last good numbers as stale; a live signal upserts one window.

use clauseo_protocol::{
    AccountLimits, AgentKind, AtLimitBehaviour, Billing, Finite, LimitSource, LimitStopReason,
    LimitWindow, StaleReason, Timestamp, UnavailableReason,
};

use crate::model::{AgentState, LiveSignal, LiveStatus, Row, kind_rank};
use crate::policy::{Action, kind_rules};
use crate::readers::{AccountRead, ReadFailure, ReadOutcome, looks_like_email};
use crate::words::claude_live_window;

/// Merges the outcome of a read of `state`'s agent, at `now`.
pub(crate) fn apply_read(
    state: &mut AgentState,
    outcome: ReadOutcome,
    now: Timestamp,
) -> Option<Action> {
    match outcome {
        ReadOutcome::Usage {
            limits,
            reached,
            account_read,
        } => {
            if limits.agent == state.agent {
                let limits = match account_read {
                    AccountRead::Read => limits,
                    AccountRead::NotRead => keep_email(state, limits),
                };
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
            let flipped = !state.method_missing;
            state.method_missing = true;
            let source_changed = failed_read(state);
            (flipped || source_changed).then(|| notice(state))
        }
        ReadOutcome::SignedOut => signed_out(state).then_some(Action::ViewChanged),
        ReadOutcome::Failed(failure) => {
            log_failure(state.agent, &failure);
            failed_read(state).then(|| notice(state))
        }
    }
}

/// Forgets the account in memory: no account, no windows, the sign-in-again state
/// (`Unavailable { MissingScope }`). Nothing is stored, so no `Changed` goes out, and an earlier
/// account's stored entry stays as it is. Returns whether the view changed.
fn signed_out(state: &mut AgentState) -> bool {
    let source = LimitSource::Unavailable {
        reason: UnavailableReason::MissingScope,
    };
    let unchanged = state.account.is_none()
        && state.plan.is_none()
        && state.billing == Billing::Plan
        && state.rows.is_empty()
        && state.at_limit == AtLimitBehaviour::Pauses
        && state.source == source
        && state.reached.is_none();
    let reads = std::mem::take(&mut state.reads);
    *state = AgentState {
        source,
        reads,
        ..AgentState::loading(state.agent)
    };
    !unchanged
}

/// `limits` from a read that did not read the account (`AccountRead::NotRead`, so the plan ref),
/// with the account ref `state` holds when that is an email for the same identity: a plan
/// account then and now, with the same named plan. Otherwise `limits` as they are: nothing was
/// known, or the kind or the plan changed. An account that really has no email comes with
/// `AccountRead::Read` and never reaches here.
pub(crate) fn keep_email(state: &AgentState, limits: AccountLimits) -> AccountLimits {
    let same = state.billing == Billing::Plan
        && limits.billing == Billing::Plan
        && state.plan.is_some()
        && state.plan == limits.plan;
    match &state.account {
        Some(account) if same && looks_like_email(&account.0) => AccountLimits {
            account: account.clone(),
            ..limits
        },
        _ => limits,
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
/// `Unavailable { ReadFailed }` when there are none. `NotApplicable` and
/// `Unavailable { MissingScope }` stay: a passing failure must not erase the sign-in
/// explanation. Rows keep their flags, so a row past its reset shows no old percentage.
/// Returns whether the source changed.
fn failed_read(state: &mut AgentState) -> bool {
    let source = match state.source {
        LimitSource::NotApplicable
        | LimitSource::Unavailable {
            reason: UnavailableReason::MissingScope,
        } => return false,
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
///
/// With no account known, a signal is dropped only while the first read runs (it will name the
/// account and replace everything) and when the agent is signed out (it has no login, so no
/// signal can come). After a failed first read or `MethodMissing` it applies, the source turns
/// `LiveOnly`, and only the view is published: there is no account to store it under.
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
    if state.account.is_none() && drops_live_without_account(state) {
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

/// Whether a state with no account drops a live signal: while its first read runs, or when it is
/// signed out (`Unavailable { MissingScope }`).
fn drops_live_without_account(state: &AgentState) -> bool {
    match state.source {
        LimitSource::Loading => state.reads.in_flight,
        LimitSource::Unavailable {
            reason: UnavailableReason::MissingScope,
        } => true,
        _ => false,
    }
}

/// Applies `signal` to `row`'s window; returns whether anything changed, the rejected flag
/// included. A rejection is a flag, never a figure: the percentage stays as the agent sent it.
/// A later `allowed` or `allowed_warning` for the window clears the flag.
fn upsert(row: &mut Row, signal: &LiveSignal) -> bool {
    let used_percent = signal.used_percent.unwrap_or(row.window.used_percent);
    let resets_at = signal.resets_at.or(row.window.resets_at);
    let (warning, rejected) = match signal.status {
        LiveStatus::Allowed => (false, false),
        LiveStatus::Warning => (true, false),
        LiveStatus::Rejected => (row.window.warning, true),
    };
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

/// A row for a window no read named yet: only for a kind whose rules allow it (Claude's 5-hour
/// and weekly windows), and only with a figure, or rejected: a rejection without a figure is at
/// the limit, so its bar is 100. That made-up 100% is stored with the account, and the next good
/// read replaces it. Anything else (an `overage` bucket) waits for a read to name it.
fn live_row(agent: AgentKind, id: String, signal: &LiveSignal, now: Timestamp) -> Option<Row> {
    if !kind_rules(agent).live_rows {
        return None;
    }
    let (kind, label) = claude_live_window(&id)?;
    let used_percent = match (signal.used_percent, signal.status) {
        (Some(used), _) => used,
        (None, LiveStatus::Rejected) => Finite::new(100.0)?,
        (None, LiveStatus::Allowed | LiveStatus::Warning) => return None,
    };
    let window = LimitWindow {
        id,
        kind,
        label: label.to_owned(),
        used_percent,
        resets_at: signal.resets_at,
        duration_mins: None,
        warning: signal.status == LiveStatus::Warning,
        model: None,
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

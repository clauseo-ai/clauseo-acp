//! The view: `AgentState` mapped to `AgentLimitsView` at a given `now`. Stale is worked out
//! here from `now`; the stored limits never change for it.

pub(crate) mod at_limit_words;

use clauseo_protocol::{
    AgentLimitsView, AtLimitBehaviour, Billing, BillingKind, Freshness, LimitMeter, LimitSource,
    LimitStopReason, LimitWindowView, MeterTone, Timestamp, UnavailableReason, WindowFreshness,
};

use crate::model::{AgentState, Row, STALE_AFTER_MS};
use crate::policy::{kind_rules, next_read_at};

/// Used from this percentage, a window's tone is `Near`.
const NEAR_PERCENT: f64 = 75.0;
/// Used from this percentage, a window's tone is `AtLimit`.
const AT_LIMIT_PERCENT: f64 = 100.0;

/// `state`'s entry in the view at `now`.
pub(crate) fn agent_view(state: &AgentState, now: Timestamp) -> AgentLimitsView {
    let rows = state.visible_rows();
    let meter = tightest(rows.iter().filter(|row| !row.reset_passed)).map(|row| LimitMeter {
        tone: row_tone(row),
        used_percent: row.window.used_percent,
        window_label: row.window.label.clone(),
        resets_at: row.window.resets_at,
    });
    AgentLimitsView {
        plan: state.plan.as_ref().map(|plan| plan.0.clone()),
        billing: match state.billing {
            Billing::Plan => BillingKind::Plan,
            Billing::PayAsYouGo { .. } => BillingKind::PayAsYouGo,
        },
        meter,
        windows: rows
            .iter()
            .map(|row| row_view(row, &state.source, now))
            .collect(),
        at_limit_text: at_limit_text(state).to_owned(),
        freshness: freshness(&state.source, now),
        checked_at: match state.source {
            LimitSource::Fresh { checked_at } | LimitSource::Stale { checked_at, .. } => {
                Some(checked_at)
            }
            LimitSource::LiveOnly { updated_at } => Some(updated_at),
            _ => None,
        },
        next_read_at: next_read_at(state),
        provider: match state.billing {
            Billing::Plan => None,
            Billing::PayAsYouGo { via } => Some(via),
        },
    }
}

/// The tightest row: the highest tone, then the highest percentage, then the earlier row.
/// Callers pass only rows that have a percentage (none past its reset).
pub(crate) fn tightest<'a>(rows: impl Iterator<Item = &'a Row>) -> Option<&'a Row> {
    rows.fold(None, |best: Option<&Row>, row| match best {
        Some(best) if tightness(row) <= tightness(best) => Some(best),
        _ => Some(row),
    })
}

/// How tight a row is, for [`tightest`].
fn tightness(row: &Row) -> (u8, clauseo_protocol::Finite) {
    let rank = match row_tone(row) {
        MeterTone::Neutral => 0,
        MeterTone::Near => 1,
        MeterTone::AtLimit => 2,
    };
    (rank, row.window.used_percent)
}

/// A row's tone. Never red: a limit is not an error.
fn row_tone(row: &Row) -> MeterTone {
    let used = row.window.used_percent.get();
    if row.reset_passed {
        MeterTone::Neutral
    } else if row.rejected || used >= AT_LIMIT_PERCENT {
        MeterTone::AtLimit
    } else if used >= NEAR_PERCENT || row.window.warning {
        MeterTone::Near
    } else {
        MeterTone::Neutral
    }
}

/// One row of the view.
fn row_view(row: &Row, source: &LimitSource, now: Timestamp) -> LimitWindowView {
    let freshness = if row.reset_passed {
        WindowFreshness::ResetPassed
    } else if matches!(source, LimitSource::Stale { .. }) || now.as_millis() >= row.stale_at_ms() {
        WindowFreshness::Stale
    } else {
        WindowFreshness::Fresh
    };
    LimitWindowView {
        id: row.window.id.clone(),
        kind: row.window.kind,
        label: row.window.label.clone(),
        model: row.window.model.clone(),
        used_percent: (!row.reset_passed).then_some(row.window.used_percent),
        tone: row_tone(row),
        resets_at: row.window.resets_at,
        freshness,
    }
}

/// How fresh an agent's numbers are at `now`.
fn freshness(source: &LimitSource, now: Timestamp) -> Freshness {
    match source {
        LimitSource::Fresh { checked_at } => {
            if now.as_millis().saturating_sub(checked_at.as_millis()) < STALE_AFTER_MS {
                Freshness::Fresh
            } else {
                Freshness::Stale
            }
        }
        LimitSource::Stale { .. } => Freshness::Stale,
        LimitSource::Loading => Freshness::Loading,
        LimitSource::Unavailable {
            reason: UnavailableReason::MissingScope,
        } => Freshness::MissingScope,
        LimitSource::Unavailable { .. } | LimitSource::Unknown { .. } => Freshness::Unavailable,
        LimitSource::NotApplicable => Freshness::NotApplicable,
        LimitSource::LiveOnly { .. } => Freshness::LiveOnly,
    }
}

/// What happens at the limit, in words.
fn at_limit_text(state: &AgentState) -> &'static str {
    if matches!(state.billing, Billing::PayAsYouGo { .. }) {
        return at_limit_words::PAY_AS_YOU_GO;
    }
    match state.reached {
        Some(LimitStopReason::WorkspaceCredits) => return at_limit_words::WORKSPACE_CREDITS,
        Some(LimitStopReason::WorkspaceSpendCap) => return at_limit_words::WORKSPACE_SPEND_CAP,
        _ => {}
    }
    match state.at_limit {
        AtLimitBehaviour::ExtraUsage { enabled: true } => at_limit_words::EXTRA_USAGE,
        AtLimitBehaviour::Credits { has_credits: true } => at_limit_words::CREDITS,
        _ => kind_rules(state.agent).pauses_words,
    }
}

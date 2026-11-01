//! `at_limit`: which limit an agent is at, for the host's at-limit stop.

use clauseo_protocol::{LimitSource, LimitStopReason, Timestamp};

use crate::model::{AgentState, LimitHit, Row};
use crate::view::tightest;

/// The limit `state`'s agent is at, at `now`, if any. Rows past their reset never count:
/// those a sweep marked, and those whose reset time is `now` or earlier, since no sweep runs
/// while nobody watches. A reached plan window ends with its rows, once every shown row is past
/// its reset; credits and the spend cap stand. An agent with no account still has hits from its
/// shown rows (`LiveOnly` after a failed read or `MethodMissing`), so the host gets the reset time
/// of the stopped turn; with no row and no reached limit (which only a good read sets, with the
/// account), the answer is `None`.
pub(crate) fn at_limit(state: &AgentState, now: Timestamp) -> Option<LimitHit> {
    if state.source == LimitSource::NotApplicable {
        return None;
    }
    let current = || {
        state.visible_rows().iter().filter(|row| {
            !row.reset_passed && row.window.resets_at.is_none_or(|resets_at| resets_at > now)
        })
    };
    let at_limit_row =
        tightest(current().filter(|row| row.rejected || row.window.used_percent.get() >= 100.0));
    match state.reached {
        Some(reason @ (LimitStopReason::WorkspaceCredits | LimitStopReason::WorkspaceSpendCap)) => {
            Some(hit(at_limit_row, reason))
        }
        _ if at_limit_row.is_some() => Some(hit(at_limit_row, LimitStopReason::PlanWindow)),
        // A plan window resets with time: once every shown row is past its reset, the reached
        // limit the read gave has passed with them (#204). With no row there is nothing to time
        // it by, so it stands until the next read.
        Some(_) if !state.visible_rows().is_empty() && current().next().is_none() => None,
        Some(_) => Some(hit(tightest(current()), LimitStopReason::PlanWindow)),
        None => None,
    }
}

/// A hit on `row`, or on no window.
fn hit(row: Option<&Row>, reason: LimitStopReason) -> LimitHit {
    LimitHit {
        window: row.map(|row| row.window.id.clone()),
        resets_at: row.and_then(|row| row.window.resets_at),
        reason,
    }
}

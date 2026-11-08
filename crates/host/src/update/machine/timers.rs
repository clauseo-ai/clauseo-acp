//! The machine's timers (the first check, the interval, the managed day) and its once-a-day
//! counts.

use clauseo_counts::schema::UpdateResult;
use clauseo_protocol::{Timestamp, UpdateState};

use super::{DAY, Effect, MINUTE, Machine};

pub(super) fn timer_fired(m: &mut Machine, now: Timestamp, fx: &mut Vec<Effect>) {
    // Cleared by a check that started on command: stale.
    let Some(due) = m.timer else {
        return;
    };
    // Early (the wall clock stepped back under the actor's one-shot timer): arm it again,
    // or no check would ever run. `due` is later than `now`, so this cannot spin.
    if now < due {
        return fx.push(Effect::ArmTimer { at: due });
    }
    m.timer = None;
    if m.may_check() {
        m.begin_check(false, fx);
    } else if matches!(m.state, UpdateState::Managed { .. }) {
        count_managed(m, now, fx);
    }
}

/// Counts `Managed` at most once a day, and arms the timer for the next day.
pub(super) fn count_managed(m: &mut Machine, now: Timestamp, fx: &mut Vec<Effect>) {
    if once_a_day(&mut m.managed_counted, now) {
        fx.push(Effect::Count(UpdateResult::Managed));
    }
    arm(m, later(now, DAY), fx);
}

/// 1 to 10 minutes after start, or after leaving `managed`.
pub(super) fn arm_first_check(m: &mut Machine, now: Timestamp, fx: &mut Vec<Effect>) {
    let delay = MINUTE + m.jitter.below(9 * MINUTE + 1);
    arm(m, later(now, delay), fx);
}

/// 60 minutes ± 10 after a check ended.
pub(super) fn arm_interval(m: &mut Machine, now: Timestamp, fx: &mut Vec<Effect>) {
    let delay = 50 * MINUTE + m.jitter.below(20 * MINUTE + 1);
    arm(m, later(now, delay), fx);
}

fn arm(m: &mut Machine, at: Timestamp, fx: &mut Vec<Effect>) {
    m.timer = Some(at);
    fx.push(Effect::ArmTimer { at });
}

/// Whether a once-a-day count is due at `now` (none yet, or the last a day or more ago); when
/// it is, `now` becomes the last.
pub(super) fn once_a_day(last: &mut Option<Timestamp>, now: Timestamp) -> bool {
    let due = last.is_none_or(|at| now.as_millis().saturating_sub(at.as_millis()) >= DAY);
    if due {
        *last = Some(now);
    }
    due
}

/// `now` plus `millis`, or `now` past the protocol's range (some 285,000 years away).
fn later(now: Timestamp, millis: i64) -> Timestamp {
    Timestamp::new(now.as_millis().saturating_add(millis)).unwrap_or(now)
}

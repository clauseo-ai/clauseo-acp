//! The rejected flag: a live `rejected` marks a window at its limit, and a good read, a later
//! `allowed` or `allowed_warning` signal for that window, or its passed reset clears it.

use clauseo_limits::{Action, Input, Limits, LiveStatus, ReadOutcome};
use clauseo_protocol::{AgentKind, MeterTone, WindowFreshness};

use crate::support::{
    account, at, changed, done, five_hour, live, live_input, pct, read_once, resetting, row,
    seven_day, t0, view_of,
};

const CLAUDE: AgentKind = AgentKind::Claude;

/// A model whose first read gave the 5-hour window at 60%, resetting at `resets_at`, and whose
/// 5-hour window a live signal then rejected at minute 1.
fn rejected_at_60(resets_at: clauseo_protocol::Timestamp) -> Limits {
    let mut model = read_once(account(
        CLAUDE,
        vec![resetting(five_hour(60.0), resets_at), seven_day(20.0)],
    ));
    let actions = model.input(
        live_input(CLAUDE, live("five_hour", LiveStatus::Rejected, None, None)),
        at(1),
    );
    assert_eq!(changed(&actions).windows[0].used_percent, pct(60.0));
    assert_eq!(
        row(&view_of(&model, CLAUDE, at(1)), "five_hour").tone,
        MeterTone::AtLimit
    );
    model
}

#[test]
fn allowed_signal_clears_the_rejected_flag_and_counts_as_a_change() {
    let mut model = rejected_at_60(at(180));
    let allowed = live("five_hour", LiveStatus::Allowed, None, None);
    let actions = model.input(live_input(CLAUDE, allowed), at(2));
    assert_eq!(
        changed(&actions).windows[0].used_percent,
        pct(60.0),
        "only the flag changed, and that is a change"
    );
    let view = view_of(&model, CLAUDE, at(2));
    assert_eq!(row(&view, "five_hour").tone, MeterTone::Neutral);
    assert_eq!(model.at_limit(CLAUDE, at(2)), None);
}

#[test]
fn warning_signal_clears_the_rejected_flag() {
    let mut model = rejected_at_60(at(180));
    let warning = live("five_hour", LiveStatus::Warning, None, None);
    let actions = model.input(live_input(CLAUDE, warning), at(2));
    assert!(changed(&actions).windows[0].warning);
    let view = view_of(&model, CLAUDE, at(2));
    assert_eq!(row(&view, "five_hour").tone, MeterTone::Near);
    assert_eq!(model.at_limit(CLAUDE, at(2)), None);
}

#[test]
fn passed_reset_clears_the_rejected_flag() {
    let mut model = rejected_at_60(at(10));
    model.input(Input::Deadline, at(10));
    assert_eq!(
        row(&view_of(&model, CLAUDE, at(10)), "five_hour").freshness,
        WindowFreshness::ResetPassed
    );
    let repeat = live("five_hour", LiveStatus::Allowed, Some(60.0), Some(at(10)));
    assert_eq!(
        model.input(live_input(CLAUDE, repeat), at(11)),
        vec![],
        "the flag is already clear, so the old figures change nothing"
    );
    let view = view_of(&model, CLAUDE, at(11));
    assert_eq!(
        row(&view, "five_hour").freshness,
        WindowFreshness::ResetPassed
    );
    assert_eq!(row(&view, "five_hour").used_percent, None);
}

#[test]
fn rejected_reset_then_allowed_after_method_missing_is_not_at_the_limit() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    model.input(done(CLAUDE, ReadOutcome::MethodMissing), t0());
    let rejected = live("five_hour", LiveStatus::Rejected, None, Some(at(10)));
    model.input(live_input(CLAUDE, rejected), at(1));
    model.input(Input::Watching { any: true }, at(11));
    model.input(Input::Deadline, at(11));
    let allowed = live("five_hour", LiveStatus::Allowed, Some(3.0), Some(at(12)));
    assert_eq!(
        model.input(live_input(CLAUDE, allowed), at(11)),
        vec![Action::ViewChanged]
    );
    let view = view_of(&model, CLAUDE, at(11));
    assert_ne!(row(&view, "five_hour").tone, MeterTone::AtLimit);
    assert_eq!(row(&view, "five_hour").used_percent, Some(pct(3.0)));
    assert_eq!(model.at_limit(CLAUDE, at(11)), None);
}

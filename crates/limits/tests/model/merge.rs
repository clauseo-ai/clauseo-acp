//! The merge: live signals upsert windows, a good read replaces everything, a failed read keeps
//! the last good numbers as stale.

use clauseo_limits::{Action, Input, Limits, LiveSignal, LiveStatus, ReadFailure, ReadOutcome};
use clauseo_protocol::{
    AgentKind, Billing, BillingProvider, Freshness, LimitSource, MeterTone, StaleReason,
    UnavailableReason, WindowKind,
};

use crate::support::{
    MINUTE, account, at, at_ms, changed, done, five_hour, live, live_input, pct, read_once,
    read_with, resetting, row, seven_day, t0, usage, view_of, window,
};

const CLAUDE: AgentKind = AgentKind::Claude;

/// A model whose first read gave the 5-hour window at `used` percent, resetting in 3 hours.
fn claude_at(used: f64) -> Limits {
    read_once(account(
        CLAUDE,
        vec![resetting(five_hour(used), at(180)), seven_day(20.0)],
    ))
}

/// Starts a second read of Claude at `now`.
fn try_again(model: &mut Limits, now: clauseo_protocol::Timestamp) {
    let actions = model.input(
        Input::TryAgain {
            agent: Some(CLAUDE),
        },
        now,
    );
    assert_eq!(actions, vec![Action::Read(CLAUDE)]);
}

#[test]
fn live_signal_upserts_the_window_with_its_id() {
    let mut model = claude_at(10.0);
    let actions = model.input(
        live_input(
            CLAUDE,
            live("five_hour", LiveStatus::Allowed, Some(30.0), None),
        ),
        at(1),
    );
    let limits = changed(&actions);
    assert_eq!(limits.windows.len(), 2);
    assert_eq!(limits.windows[0].id, "five_hour");
    assert_eq!(limits.windows[0].used_percent, pct(30.0));
    assert_eq!(
        limits.windows[1].used_percent,
        pct(20.0),
        "the other window is untouched"
    );
    assert_eq!(
        limits.source,
        LimitSource::Fresh { checked_at: t0() },
        "a live update keeps Fresh"
    );
}

#[test]
fn live_signal_with_a_new_reset_time_replaces_the_old_one() {
    let mut model = claude_at(10.0);
    let actions = model.input(
        live_input(
            CLAUDE,
            live("five_hour", LiveStatus::Allowed, None, Some(at(200))),
        ),
        at(1),
    );
    assert_eq!(changed(&actions).windows[0].resets_at, Some(at(200)));
    assert_eq!(
        changed(&actions).windows[0].used_percent,
        pct(10.0),
        "no figure keeps the old one"
    );
}

#[test]
fn live_signal_without_a_reset_time_keeps_the_old_one() {
    let mut model = claude_at(10.0);
    let actions = model.input(
        live_input(
            CLAUDE,
            live("five_hour", LiveStatus::Allowed, Some(40.0), None),
        ),
        at(1),
    );
    let limits = changed(&actions);
    assert_eq!(limits.windows[0].used_percent, pct(40.0));
    assert_eq!(limits.windows[0].resets_at, Some(at(180)));
}

#[test]
fn live_signal_that_changes_nothing_is_ignored_and_bumps_nothing() {
    let mut model = claude_at(10.0);
    let same = live("five_hour", LiveStatus::Allowed, Some(10.0), Some(at(180)));
    assert_eq!(model.input(live_input(CLAUDE, same), at(16)), vec![]);
    let view = view_of(&model, CLAUDE, at(16));
    assert_eq!(
        row(&view, "five_hour").freshness,
        clauseo_protocol::WindowFreshness::Stale,
        "the row's update time was not bumped"
    );
}

#[test]
fn live_signal_without_a_window_is_ignored() {
    let mut model = claude_at(10.0);
    let signal = LiveSignal {
        window: None,
        status: LiveStatus::Rejected,
        used_percent: None,
        resets_at: None,
    };
    assert_eq!(model.input(live_input(CLAUDE, signal), at(1)), vec![]);
}

#[test]
fn live_signal_never_revives_not_applicable() {
    let limits = clauseo_protocol::AccountLimits {
        billing: Billing::PayAsYouGo {
            via: BillingProvider::Anthropic,
        },
        source: LimitSource::NotApplicable,
        ..account(CLAUDE, Vec::new())
    };
    let mut model = read_once(limits);
    let actions = model.input(
        live_input(
            CLAUDE,
            live("five_hour", LiveStatus::Warning, Some(80.0), None),
        ),
        at(1),
    );
    assert_eq!(actions, vec![]);
    let view = view_of(&model, CLAUDE, at(1));
    assert_eq!(view.freshness, Freshness::NotApplicable);
    assert_eq!(view.windows, vec![]);
}

#[test]
fn live_signal_before_the_first_read_ends_is_ignored() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    let actions = model.input(
        live_input(
            CLAUDE,
            live("five_hour", LiveStatus::Allowed, Some(30.0), None),
        ),
        at(1),
    );
    assert_eq!(actions, vec![]);
    assert_eq!(view_of(&model, CLAUDE, at(1)).freshness, Freshness::Loading);
}

#[test]
fn rejected_signal_marks_the_window_rejected_until_the_next_good_read() {
    let mut model = claude_at(60.0);
    let actions = model.input(
        live_input(CLAUDE, live("five_hour", LiveStatus::Rejected, None, None)),
        at(1),
    );
    assert_eq!(
        changed(&actions).windows[0].used_percent,
        pct(60.0),
        "the figure stays as sent"
    );
    let view = view_of(&model, CLAUDE, at(1));
    assert_eq!(row(&view, "five_hour").tone, MeterTone::AtLimit);
    assert_eq!(row(&view, "five_hour").used_percent, Some(pct(60.0)));
    let hit = model
        .at_limit(CLAUDE)
        .expect("a rejected window is at its limit");
    assert_eq!(hit.window.as_deref(), Some("five_hour"));

    let allowed = live("five_hour", LiveStatus::Allowed, Some(61.0), None);
    model.input(live_input(CLAUDE, allowed), at(2));
    assert_eq!(
        row(&view_of(&model, CLAUDE, at(2)), "five_hour").tone,
        MeterTone::AtLimit
    );

    try_again(&mut model, at(3));
    model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![five_hour(60.0)]))),
        at(3),
    );
    assert_eq!(
        row(&view_of(&model, CLAUDE, at(3)), "five_hour").tone,
        MeterTone::Neutral
    );
    assert_eq!(model.at_limit(CLAUDE), None);
}

#[test]
fn failed_read_keeps_the_last_good_numbers_as_stale_read_failed() {
    let mut model = claude_at(40.0);
    try_again(&mut model, at(6));
    let actions = model.input(
        done(CLAUDE, ReadOutcome::Failed(ReadFailure::TimedOut)),
        at(7),
    );
    let limits = changed(&actions);
    assert_eq!(
        limits.source,
        LimitSource::Stale {
            checked_at: t0(),
            reason: StaleReason::ReadFailed
        }
    );
    assert_eq!(limits.windows[0].used_percent, pct(40.0));
    let view = view_of(&model, CLAUDE, at(7));
    assert_eq!(view.freshness, Freshness::Stale);
    assert_eq!(row(&view, "five_hour").used_percent, Some(pct(40.0)));
}

#[test]
fn second_failed_read_in_a_row_changes_nothing_visible() {
    let mut model = claude_at(40.0);
    try_again(&mut model, at(6));
    model.input(
        done(CLAUDE, ReadOutcome::Failed(ReadFailure::TimedOut)),
        at(7),
    );
    try_again(&mut model, at(8));
    let exited = ReadFailure::Exited {
        code: Some(1),
        stderr_tail: "boom".to_owned(),
    };
    assert_eq!(
        model.input(done(CLAUDE, ReadOutcome::Failed(exited)), at(9)),
        vec![]
    );
}

#[test]
fn failed_first_read_without_numbers_is_unavailable_read_failed() {
    let model = read_with(ReadOutcome::Failed(ReadFailure::Malformed(
        "not json".to_owned(),
    )));
    let view = view_of(&model, CLAUDE, at(1));
    assert_eq!(view.freshness, Freshness::Unavailable);
    assert_eq!(view.windows, vec![]);
}

#[test]
fn failed_first_read_publishes_the_view_only() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    let outcome = ReadOutcome::Failed(ReadFailure::LaunchFailed("no node".to_owned()));
    assert_eq!(
        model.input(done(CLAUDE, outcome), at(1)),
        vec![Action::ViewChanged]
    );
}

#[test]
fn good_read_replaces_live_updates_that_arrived_during_it() {
    let mut model = claude_at(10.0);
    try_again(&mut model, at(6));
    model.input(
        live_input(
            CLAUDE,
            live("five_hour", LiveStatus::Warning, Some(50.0), None),
        ),
        at(7),
    );
    let actions = model.input(
        done(CLAUDE, usage(account(CLAUDE, vec![five_hour(20.0)]))),
        at(8),
    );
    let limits = changed(&actions);
    assert_eq!(limits.windows.len(), 1);
    assert_eq!(limits.windows[0].used_percent, pct(20.0));
    assert!(!limits.windows[0].warning);
    assert_eq!(limits.source, LimitSource::Fresh { checked_at: at(8) });
}

#[test]
fn good_read_sorts_windows_session_weekly_model_other() {
    let windows = vec![
        window("extra", WindowKind::Other, 1.0),
        window("opus", WindowKind::Model, 2.0),
        seven_day(3.0),
        five_hour(4.0),
    ];
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    let actions = model.input(done(CLAUDE, usage(account(CLAUDE, windows))), t0());
    let ids: Vec<&str> = changed(&actions)
        .windows
        .iter()
        .map(|w| w.id.as_str())
        .collect();
    assert_eq!(ids, ["five_hour", "seven_day", "opus", "extra"]);
}

#[test]
fn good_read_keeps_missing_scope_as_the_reader_set_it() {
    let source = LimitSource::Unavailable {
        reason: UnavailableReason::MissingScope,
    };
    let limits = clauseo_protocol::AccountLimits {
        source: source.clone(),
        ..account(CLAUDE, vec![])
    };
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    assert_eq!(
        changed(&model.input(done(CLAUDE, usage(limits)), at(1))).source,
        source
    );
}

#[test]
fn read_for_another_agent_is_a_failed_read() {
    let (mut claude, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    let codex_figures = usage(account(AgentKind::Codex, vec![five_hour(5.0)]));
    let actions = claude.input(done(CLAUDE, codex_figures), at(1));
    assert_eq!(actions, vec![Action::ViewChanged]);
    let view = view_of(&claude, CLAUDE, at(1));
    assert_eq!(view.freshness, Freshness::Unavailable);
    assert_eq!(view.windows, vec![]);
}

#[test]
fn overage_signal_before_any_read_named_it_is_dropped() {
    let mut model = read_once(account(CLAUDE, vec![seven_day(20.0)]));
    let actions = model.input(
        live_input(
            CLAUDE,
            live("overage", LiveStatus::Rejected, Some(100.0), None),
        ),
        at(1),
    );
    assert_eq!(actions, vec![]);
    assert!(model.at_limit(CLAUDE).is_none());
}

#[test]
fn five_hour_signal_before_any_read_named_it_is_added_in_order() {
    let mut model = read_once(account(CLAUDE, vec![seven_day(20.0)]));
    let signal = live("five_hour", LiveStatus::Warning, Some(30.0), Some(at(100)));
    let actions = model.input(live_input(CLAUDE, signal), at(1));
    let limits = changed(&actions);
    let ids: Vec<&str> = limits.windows.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["five_hour", "seven_day"]);
    assert_eq!(limits.windows[0].kind, WindowKind::Session);
    assert_eq!(limits.windows[0].label, "5-hour limit");
    assert_eq!(limits.windows[0].resets_at, Some(at(100)));
    assert!(limits.windows[0].warning);
}

#[test]
fn five_hour_signal_without_a_figure_is_not_added() {
    let mut model = read_once(account(CLAUDE, vec![seven_day(20.0)]));
    let actions = model.input(
        live_input(CLAUDE, live("five_hour", LiveStatus::Allowed, None, None)),
        at(1),
    );
    assert_eq!(actions, vec![]);
}

#[test]
fn method_missing_with_earlier_numbers_is_stale_read_failed_then_live_only() {
    let mut model = claude_at(10.0);
    try_again(&mut model, at(6));
    let actions = model.input(done(CLAUDE, ReadOutcome::MethodMissing), at(7));
    assert_eq!(
        changed(&actions).source,
        LimitSource::Stale {
            checked_at: t0(),
            reason: StaleReason::ReadFailed
        }
    );
    assert_eq!(view_of(&model, CLAUDE, at(7)).freshness, Freshness::Stale);

    let actions = model.input(
        live_input(
            CLAUDE,
            live("five_hour", LiveStatus::Allowed, Some(15.0), None),
        ),
        at(8),
    );
    assert_eq!(
        changed(&actions).source,
        LimitSource::LiveOnly { updated_at: at(8) }
    );
    let view = view_of(&model, CLAUDE, at(8));
    assert_eq!(view.freshness, Freshness::LiveOnly);
    assert_eq!(view.checked_at, Some(at(8)));
}

#[test]
fn method_missing_without_numbers_is_unavailable_read_failed_then_live_only() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    let actions = model.input(done(CLAUDE, ReadOutcome::MethodMissing), at(1));
    assert_eq!(actions, vec![Action::ViewChanged]);
    assert_eq!(
        view_of(&model, CLAUDE, at(1)).freshness,
        Freshness::Unavailable
    );

    let signal = live("five_hour", LiveStatus::Allowed, Some(30.0), Some(at(90)));
    let actions = model.input(live_input(CLAUDE, signal), at_ms(MINUTE + 1));
    assert_eq!(
        actions,
        vec![Action::ViewChanged],
        "no account yet, so nothing to persist"
    );
    let view = view_of(&model, CLAUDE, at(2));
    assert_eq!(view.freshness, Freshness::LiveOnly);
    assert_eq!(view.billing, clauseo_protocol::BillingKind::Plan);
    assert_eq!(
        view.at_limit_text,
        "When you reach a limit, Claude pauses until it resets."
    );
    assert_eq!(row(&view, "five_hour").used_percent, Some(pct(30.0)));
}

//! The view at `now`: tones, freshness, the meter and the at-limit line.

use clauseo_limits::{Input, Limits, LiveStatus, ReadOutcome};
use clauseo_protocol::{
    AccountLimits, AgentKind, AtLimitBehaviour, Billing, BillingKind, BillingProvider, Freshness,
    LimitSource, LimitStopReason, LimitWindow, MeterTone, UnavailableReason, WindowFreshness,
    WindowKind,
};

use crate::support::{
    MINUTE, account, at, at_ms, done, five_hour, live, live_input, pct, read_once, read_with,
    resetting, row, seven_day, t0, usage, usage_reached, view_of, window,
};

const CLAUDE: AgentKind = AgentKind::Claude;
const CODEX: AgentKind = AgentKind::Codex;

/// The at-limit line of a model read once with `limits`.
fn text_of(limits: AccountLimits) -> String {
    let agent = limits.agent;
    view_of(&read_once(limits), agent, t0()).at_limit_text
}

/// The at-limit line of `agent` with `at_limit`.
fn text_with(agent: AgentKind, at_limit: AtLimitBehaviour) -> String {
    text_of(AccountLimits {
        at_limit,
        ..account(agent, vec![])
    })
}

/// The tone of a single window `window` read at the start time.
fn tone_of(window: LimitWindow) -> MeterTone {
    let view = view_of(&read_once(account(CLAUDE, vec![window])), CLAUDE, t0());
    view.windows[0].tone
}

#[test]
fn tone_is_neutral_at_74_9() {
    assert_eq!(tone_of(five_hour(74.9)), MeterTone::Neutral);
}

#[test]
fn tone_is_near_at_75() {
    assert_eq!(tone_of(five_hour(75.0)), MeterTone::Near);
}

#[test]
fn tone_is_at_limit_at_100() {
    assert_eq!(tone_of(five_hour(99.9)), MeterTone::Near);
    assert_eq!(tone_of(five_hour(100.0)), MeterTone::AtLimit);
}

#[test]
fn tone_is_near_when_the_agent_warns() {
    assert_eq!(
        tone_of(LimitWindow {
            warning: true,
            ..five_hour(10.0)
        }),
        MeterTone::Near
    );
    let mut model = read_once(account(CLAUDE, vec![five_hour(10.0)]));
    model.input(
        live_input(CLAUDE, live("five_hour", LiveStatus::Warning, None, None)),
        at(1),
    );
    assert_eq!(
        row(&view_of(&model, CLAUDE, at(1)), "five_hour").tone,
        MeterTone::Near
    );
}

#[test]
fn fresh_under_15_minutes_stale_at_exactly_15() {
    let model = read_once(account(CLAUDE, vec![five_hour(10.0)]));
    let before = view_of(&model, CLAUDE, at_ms(15 * MINUTE - 1));
    assert_eq!(before.freshness, Freshness::Fresh);
    assert_eq!(row(&before, "five_hour").freshness, WindowFreshness::Fresh);
    assert_eq!(before.checked_at, Some(t0()));
    let after = view_of(&model, CLAUDE, at(15));
    assert_eq!(after.freshness, Freshness::Stale);
    assert_eq!(row(&after, "five_hour").freshness, WindowFreshness::Stale);
    assert_eq!(
        row(&after, "five_hour").used_percent,
        Some(pct(10.0)),
        "stale keeps its numbers"
    );
}

#[test]
fn reset_passed_row_has_no_percentage_and_a_neutral_tone() {
    let mut model = read_once(account(CLAUDE, vec![resetting(five_hour(100.0), at(10))]));
    model.input(Input::Deadline, at(10));
    let view = view_of(&model, CLAUDE, at(10));
    let row = row(&view, "five_hour");
    assert_eq!(row.used_percent, None);
    assert_eq!(row.tone, MeterTone::Neutral);
    assert_eq!(row.freshness, WindowFreshness::ResetPassed);
    assert_eq!(row.resets_at, Some(at(10)));
}

#[test]
fn not_applicable_has_no_bars_and_the_pay_as_you_go_text() {
    let limits = AccountLimits {
        billing: Billing::PayAsYouGo {
            via: BillingProvider::Bedrock,
        },
        source: LimitSource::NotApplicable,
        ..account(CLAUDE, vec![five_hour(50.0)])
    };
    let view = view_of(&read_once(limits), CLAUDE, t0());
    assert_eq!(view.freshness, Freshness::NotApplicable);
    assert_eq!(view.billing, BillingKind::PayAsYouGo);
    assert_eq!(view.windows, vec![]);
    assert_eq!(view.meter, None);
    assert_eq!(view.checked_at, None);
    assert_eq!(
        view.at_limit_text,
        "This account pays as it goes, so no plan limits apply."
    );
}

#[test]
fn loading_has_its_own_freshness_and_no_bars() {
    let (model, _) = Limits::new(Vec::new(), &[CODEX], t0());
    let view = view_of(&model, CODEX, t0());
    assert_eq!(view.freshness, Freshness::Loading);
    assert_eq!(view.billing, BillingKind::Plan);
    assert_eq!(view.windows, vec![]);
    assert_eq!(view.plan, None);
}

#[test]
fn unavailable_has_its_own_freshness_and_no_bars() {
    let view = view_of(&read_with(ReadOutcome::MethodMissing), CLAUDE, t0());
    assert_eq!(view.freshness, Freshness::Unavailable);
    assert_eq!(view.windows, vec![]);
    assert_eq!(view.checked_at, None);
}

#[test]
fn missing_scope_has_its_own_freshness_and_no_bars() {
    let limits = AccountLimits {
        source: LimitSource::Unavailable {
            reason: UnavailableReason::MissingScope,
        },
        ..account(CLAUDE, vec![five_hour(10.0)])
    };
    let view = view_of(&read_once(limits), CLAUDE, t0());
    assert_eq!(view.freshness, Freshness::MissingScope);
    assert_eq!(view.windows, vec![]);
}

#[test]
fn live_only_has_its_own_freshness_with_bars() {
    let mut model = read_with(ReadOutcome::MethodMissing);
    model.input(
        live_input(
            CLAUDE,
            live("seven_day", LiveStatus::Allowed, Some(5.0), None),
        ),
        at(1),
    );
    let view = view_of(&model, CLAUDE, at(1));
    assert_eq!(view.freshness, Freshness::LiveOnly);
    assert_eq!(row(&view, "seven_day").freshness, WindowFreshness::Fresh);
    assert_eq!(row(&view, "seven_day").kind, WindowKind::Weekly);
}

#[test]
fn plan_label_and_plan_billing_are_copied() {
    let view = view_of(&read_once(account(CLAUDE, vec![])), CLAUDE, t0());
    assert_eq!(view.plan.as_deref(), Some("Max"));
    assert_eq!(view.billing, BillingKind::Plan);
}

#[test]
fn meter_is_the_tightest_window() {
    let windows = vec![
        resetting(five_hour(50.0), at(100)),
        resetting(seven_day(90.0), at(200)),
        window("opus", WindowKind::Model, 80.0),
    ];
    let view = view_of(&read_once(account(CLAUDE, windows)), CLAUDE, t0());
    let meter = view.meter.expect("a window has a percentage");
    assert_eq!(meter.window_label, "seven_day label");
    assert_eq!(meter.used_percent, pct(90.0));
    assert_eq!(meter.tone, MeterTone::Near);
    assert_eq!(meter.resets_at, Some(at(200)));
}

#[test]
fn meter_picks_a_higher_tone_before_a_higher_percentage() {
    let mut model = read_once(account(CLAUDE, vec![five_hour(40.0), seven_day(99.0)]));
    model.input(
        live_input(CLAUDE, live("five_hour", LiveStatus::Rejected, None, None)),
        at(1),
    );
    let meter = view_of(&model, CLAUDE, at(1)).meter.expect("a meter");
    assert_eq!(meter.window_label, "five_hour label");
    assert_eq!(meter.tone, MeterTone::AtLimit);
    assert_eq!(meter.used_percent, pct(40.0));
}

#[test]
fn meter_picks_the_earlier_row_on_a_tie() {
    let model = read_once(account(CLAUDE, vec![five_hour(30.0), seven_day(30.0)]));
    let meter = view_of(&model, CLAUDE, t0()).meter.expect("a meter");
    assert_eq!(meter.window_label, "five_hour label");
}

#[test]
fn meter_is_none_without_windows() {
    assert_eq!(
        view_of(&read_once(account(CLAUDE, vec![])), CLAUDE, t0()).meter,
        None
    );
}

#[test]
fn claude_pauses_text() {
    let text = text_with(CLAUDE, AtLimitBehaviour::Pauses);
    assert_eq!(
        text,
        "When you reach a limit, Claude pauses until it resets."
    );
}

#[test]
fn codex_pauses_text() {
    let text = text_with(CODEX, AtLimitBehaviour::Pauses);
    assert_eq!(
        text,
        "When you reach a limit, Codex pauses until it resets."
    );
}

#[test]
fn extra_usage_on_text() {
    assert_eq!(
        text_with(CLAUDE, AtLimitBehaviour::ExtraUsage { enabled: true }),
        "When you reach a limit, Claude continues on extra usage, billed to your Claude account."
    );
}

#[test]
fn extra_usage_off_says_claude_pauses() {
    assert_eq!(
        text_with(CLAUDE, AtLimitBehaviour::ExtraUsage { enabled: false }),
        "When you reach a limit, Claude pauses until it resets."
    );
}

#[test]
fn credits_text() {
    assert_eq!(
        text_with(CODEX, AtLimitBehaviour::Credits { has_credits: true }),
        "When you reach a limit, Codex continues on your plan's credits."
    );
}

#[test]
fn no_credits_says_codex_pauses() {
    assert_eq!(
        text_with(CODEX, AtLimitBehaviour::Credits { has_credits: false }),
        "When you reach a limit, Codex pauses until it resets."
    );
}

#[test]
fn workspace_credits_text() {
    let (mut model, _) = Limits::new(Vec::new(), &[CODEX], t0());
    let limits = AccountLimits {
        at_limit: AtLimitBehaviour::Credits { has_credits: true },
        ..account(CODEX, vec![])
    };
    model.input(
        done(
            CODEX,
            usage_reached(limits, LimitStopReason::WorkspaceCredits),
        ),
        t0(),
    );
    assert_eq!(
        view_of(&model, CODEX, t0()).at_limit_text,
        "Your workspace is out of credits. Ask your workspace admin to add more."
    );
}

#[test]
fn workspace_spend_cap_text() {
    let (mut model, _) = Limits::new(Vec::new(), &[CODEX], t0());
    let limits = account(CODEX, vec![]);
    model.input(
        done(
            CODEX,
            usage_reached(limits, LimitStopReason::WorkspaceSpendCap),
        ),
        t0(),
    );
    assert_eq!(
        view_of(&model, CODEX, t0()).at_limit_text,
        "Your workspace reached its spend limit. Ask your workspace admin to raise it."
    );
    model.input(Input::TryAgain { agent: Some(CODEX) }, at(1));
    model.input(done(CODEX, usage(account(CODEX, vec![]))), at(1));
    assert_eq!(
        view_of(&model, CODEX, at(1)).at_limit_text,
        "When you reach a limit, Codex pauses until it resets.",
        "the next good read without the reason clears it"
    );
}

#[test]
fn pay_as_you_go_view_names_its_provider() {
    let limits = AccountLimits {
        billing: Billing::PayAsYouGo {
            via: BillingProvider::Bedrock,
        },
        source: LimitSource::NotApplicable,
        ..account(CLAUDE, vec![five_hour(50.0)])
    };
    let view = view_of(&read_once(limits), CLAUDE, t0());
    assert_eq!(view.billing, BillingKind::PayAsYouGo);
    assert_eq!(view.provider, Some(BillingProvider::Bedrock));
}

#[test]
fn plan_view_has_no_provider() {
    let view = view_of(
        &read_once(account(CLAUDE, vec![five_hour(10.0)])),
        CLAUDE,
        t0(),
    );
    assert_eq!(view.provider, None);
}

#[test]
fn next_read_at_is_the_earliest_reset_not_yet_passed() {
    let model = read_once(account(
        CLAUDE,
        vec![
            resetting(five_hour(40.0), at(60)),
            resetting(seven_day(20.0), at(10)),
        ],
    ));
    assert_eq!(view_of(&model, CLAUDE, t0()).next_read_at, Some(at(10)));
}

#[test]
fn next_read_at_moves_past_a_reset_that_passed() {
    let mut model = read_once(account(
        CLAUDE,
        vec![
            resetting(five_hour(40.0), at(60)),
            resetting(seven_day(20.0), at(10)),
        ],
    ));
    model.input(Input::Deadline, at(10));
    assert_eq!(view_of(&model, CLAUDE, at(10)).next_read_at, Some(at(60)));
}

#[test]
fn next_read_at_is_none_once_the_last_reset_passed() {
    let mut model = read_once(account(CLAUDE, vec![resetting(five_hour(40.0), at(10))]));
    model.input(Input::Deadline, at(10));
    assert_eq!(view_of(&model, CLAUDE, at(10)).next_read_at, None);
}

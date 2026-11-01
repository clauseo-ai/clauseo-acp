//! `at_limit`: which limit an agent is at, for the host's at-limit stop.

use clauseo_limits::{Action, Input, LimitHit, Limits, LiveStatus, ReadOutcome};
use clauseo_protocol::{AgentKind, LimitStopReason};

use crate::support::{
    account, at, done, five_hour, live, live_input, read_once, resetting, seven_day, t0,
    usage_reached,
};

const CLAUDE: AgentKind = AgentKind::Claude;
const CODEX: AgentKind = AgentKind::Codex;

/// A Codex model whose first read gave `windows` and said the account reached `reached`.
fn codex_reached(windows: Vec<clauseo_protocol::LimitWindow>, reached: LimitStopReason) -> Limits {
    let (mut model, _) = Limits::new(Vec::new(), &[CODEX], t0());
    model.input(
        done(CODEX, usage_reached(account(CODEX, windows), reached)),
        t0(),
    );
    model
}

#[test]
fn window_at_100_gives_its_id_reset_time_and_plan_window() {
    let model = read_once(account(
        CLAUDE,
        vec![
            resetting(five_hour(40.0), at(60)),
            resetting(seven_day(100.0), at(600)),
        ],
    ));
    let hit = LimitHit {
        window: Some("seven_day".to_owned()),
        resets_at: Some(at(600)),
        reason: LimitStopReason::PlanWindow,
    };
    assert_eq!(model.at_limit(CLAUDE), Some(hit));
}

#[test]
fn rejected_window_gives_its_id_reset_time_and_plan_window() {
    let mut model = read_once(account(CLAUDE, vec![resetting(five_hour(70.0), at(60))]));
    model.input(
        live_input(CLAUDE, live("five_hour", LiveStatus::Rejected, None, None)),
        at(1),
    );
    let hit = LimitHit {
        window: Some("five_hour".to_owned()),
        resets_at: Some(at(60)),
        reason: LimitStopReason::PlanWindow,
    };
    assert_eq!(model.at_limit(CLAUDE), Some(hit));
}

#[test]
fn workspace_credits_gives_workspace_credits() {
    let model = codex_reached(vec![five_hour(20.0)], LimitStopReason::WorkspaceCredits);
    let hit = LimitHit {
        window: None,
        resets_at: None,
        reason: LimitStopReason::WorkspaceCredits,
    };
    assert_eq!(model.at_limit(CODEX), Some(hit));
}

#[test]
fn workspace_spend_cap_gives_workspace_spend_cap_with_the_window_at_its_limit() {
    let windows = vec![resetting(five_hour(100.0), at(30)), seven_day(50.0)];
    let model = codex_reached(windows, LimitStopReason::WorkspaceSpendCap);
    let hit = LimitHit {
        window: Some("five_hour".to_owned()),
        resets_at: Some(at(30)),
        reason: LimitStopReason::WorkspaceSpendCap,
    };
    assert_eq!(model.at_limit(CODEX), Some(hit));
}

#[test]
fn reached_plan_window_gives_plan_window_on_the_tightest_window() {
    let windows = vec![five_hour(20.0), resetting(seven_day(60.0), at(300))];
    let model = codex_reached(windows, LimitStopReason::PlanWindow);
    let hit = LimitHit {
        window: Some("seven_day".to_owned()),
        resets_at: Some(at(300)),
        reason: LimitStopReason::PlanWindow,
    };
    assert_eq!(model.at_limit(CODEX), Some(hit));
}

#[test]
fn reached_temporary_without_windows_gives_plan_window_on_no_window() {
    let model = codex_reached(vec![], LimitStopReason::Temporary);
    let hit = LimitHit {
        window: None,
        resets_at: None,
        reason: LimitStopReason::PlanWindow,
    };
    assert_eq!(model.at_limit(CODEX), Some(hit));
}

#[test]
fn no_window_at_its_limit_gives_none() {
    let model = read_once(account(CLAUDE, vec![five_hour(99.9), seven_day(10.0)]));
    assert_eq!(model.at_limit(CLAUDE), None);
}

#[test]
fn window_at_100_past_its_reset_does_not_count() {
    let mut model = read_once(account(CLAUDE, vec![resetting(five_hour(100.0), at(10))]));
    assert!(model.at_limit(CLAUDE).is_some());
    model.input(Input::Deadline, at(10));
    assert_eq!(model.at_limit(CLAUDE), None);
}

#[test]
fn not_applicable_gives_none_even_when_a_limit_was_reached() {
    let limits = clauseo_protocol::AccountLimits {
        billing: clauseo_protocol::Billing::PayAsYouGo {
            via: clauseo_protocol::BillingProvider::OpenAi,
        },
        source: clauseo_protocol::LimitSource::NotApplicable,
        ..account(CODEX, vec![five_hour(100.0)])
    };
    let (mut model, _) = Limits::new(Vec::new(), &[CODEX], t0());
    model.input(
        done(
            CODEX,
            usage_reached(limits, LimitStopReason::WorkspaceCredits),
        ),
        t0(),
    );
    assert_eq!(model.at_limit(CODEX), None);
}

#[test]
fn rejected_live_signal_without_an_account_gives_its_hit() {
    let (mut model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    model.input(done(CLAUDE, ReadOutcome::MethodMissing), t0());
    assert_eq!(model.at_limit(CLAUDE), None, "no shown row yet");
    let signal = live("five_hour", LiveStatus::Rejected, Some(90.0), Some(at(45)));
    let actions = model.input(live_input(CLAUDE, signal), at(1));
    assert_eq!(actions, vec![Action::ViewChanged], "still no account");
    let hit = LimitHit {
        window: Some("five_hour".to_owned()),
        resets_at: Some(at(45)),
        reason: LimitStopReason::PlanWindow,
    };
    assert_eq!(model.at_limit(CLAUDE), Some(hit));
}

#[test]
fn no_account_gives_none() {
    let (model, _) = Limits::new(Vec::new(), &[CLAUDE], t0());
    assert_eq!(model.at_limit(CLAUDE), None);
}

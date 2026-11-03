//! Codex's answers, from the fixtures and from hand-made edge cases, to the exact outcome.

mod prepare;
mod talk;

use clauseo_codex_appserver::{GetAccountRateLimitsResponse, GetAccountResponse};
use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, BillingProvider, Finite,
    LimitSource, LimitStopReason, LimitWindow, PlanLabel, Timestamp, WindowKind,
};
use serde_json::{Value, json};

use super::{Answers, outcome};
use crate::{ReadFailure, ReadOutcome};

/// The resets in the fixtures, in Unix seconds.
const FIVE_HOUR_RESET: i64 = 1_793_642_400;
const WEEK_RESET: i64 = 1_793_955_600;
const MONTH_RESET: i64 = 1_795_000_000;

/// A fixture's `{accountRead, rateLimitsRead}` as JSON.
pub(super) fn fixture(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// The answers a fixture holds.
pub(super) fn answers(fixture: &Value) -> Answers {
    let account: GetAccountResponse =
        serde_json::from_value(fixture["accountRead"].clone()).unwrap();
    let limits: Option<GetAccountRateLimitsResponse> = fixture
        .get("rateLimitsRead")
        .map(|limits| serde_json::from_value(limits.clone()).unwrap());
    Answers { account, limits }
}

fn read(text: &str) -> ReadOutcome {
    outcome(answers(&fixture(text)))
}

fn window(
    id: &str,
    kind: WindowKind,
    label: &str,
    used: f64,
    reset: Option<i64>,
    mins: Option<u32>,
) -> LimitWindow {
    LimitWindow {
        id: id.to_owned(),
        kind,
        label: label.to_owned(),
        used_percent: Finite::new(used).unwrap(),
        resets_at: reset.map(|secs| Timestamp::new(secs * 1000).unwrap()),
        duration_mins: mins,
        warning: false,
        model: None,
    }
}

fn session(used: f64) -> LimitWindow {
    window(
        "primary",
        WindowKind::Session,
        "5-hour limit",
        used,
        Some(FIVE_HOUR_RESET),
        Some(300),
    )
}

fn weekly(used: f64) -> LimitWindow {
    window(
        "secondary",
        WindowKind::Weekly,
        "Weekly limit",
        used,
        Some(WEEK_RESET),
        Some(10_080),
    )
}

fn plan(
    account: &str,
    plan: &str,
    windows: Vec<LimitWindow>,
    at_limit: AtLimitBehaviour,
    reached: Option<LimitStopReason>,
) -> ReadOutcome {
    ReadOutcome::Usage {
        limits: AccountLimits {
            agent: AgentKind::Codex,
            account: AccountRef(account.to_owned()),
            plan: Some(PlanLabel(plan.to_owned())),
            billing: Billing::Plan,
            windows,
            at_limit,
            source: LimitSource::Fresh {
                checked_at: Timestamp::ZERO,
            },
        },
        reached,
    }
}

pub(super) fn pay_as_you_go(account: &str, via: BillingProvider) -> ReadOutcome {
    ReadOutcome::Usage {
        limits: AccountLimits {
            agent: AgentKind::Codex,
            account: AccountRef(account.to_owned()),
            plan: None,
            billing: Billing::PayAsYouGo { via },
            windows: Vec::new(),
            at_limit: AtLimitBehaviour::Pauses,
            source: LimitSource::NotApplicable,
        },
        reached: None,
    }
}

/// The outcome `plus.json` maps to.
pub(super) fn plus_outcome() -> ReadOutcome {
    plan(
        "plus.user@example.com",
        "ChatGPT Plus",
        vec![session(25.0), weekly(40.0)],
        AtLimitBehaviour::Pauses,
        None,
    )
}

#[test]
fn plus_takes_its_rows_from_the_codex_bucket_not_the_model_bucket() {
    assert_eq!(
        read(include_str!("../../../fixtures/codex/plus.json")),
        plus_outcome()
    );
}

#[test]
fn free_has_one_monthly_window_with_or_without_its_length() {
    let monthly = |mins| {
        window(
            "primary",
            WindowKind::Other,
            "Monthly limit",
            60.0,
            Some(MONTH_RESET),
            mins,
        )
    };
    let free = |mins| {
        plan(
            "free.user@example.com",
            "ChatGPT Free",
            vec![monthly(mins)],
            AtLimitBehaviour::Pauses,
            None,
        )
    };
    assert_eq!(
        read(include_str!("../../../fixtures/codex/free.json")),
        free(Some(43_200))
    );
    assert_eq!(
        read(include_str!(
            "../../../fixtures/codex/free-no-duration.json"
        )),
        free(None)
    );
}

#[test]
fn go_with_no_email_is_the_plan_ref() {
    let monthly = window(
        "primary",
        WindowKind::Other,
        "Monthly limit",
        15.0,
        Some(MONTH_RESET),
        Some(40_320),
    );
    assert_eq!(
        read(include_str!("../../../fixtures/codex/go.json")),
        plan(
            "plan",
            "ChatGPT Go",
            vec![monthly],
            AtLimitBehaviour::Pauses,
            None
        )
    );
}

#[test]
fn api_key_and_bedrock_pay_as_they_go_and_no_account_is_signed_out() {
    assert_eq!(
        read(include_str!("../../../fixtures/codex/api-key.json")),
        pay_as_you_go("api-key", BillingProvider::OpenAi)
    );
    assert_eq!(
        read(include_str!("../../../fixtures/codex/bedrock.json")),
        pay_as_you_go("bedrock", BillingProvider::Bedrock)
    );
    assert_eq!(
        read(include_str!("../../../fixtures/codex/signed-out.json")),
        ReadOutcome::SignedOut
    );
}

#[test]
fn credits_and_the_snapshots_plan_win() {
    assert_eq!(
        read(include_str!("../../../fixtures/codex/credits.json")),
        plan(
            "credits.user@example.com",
            "ChatGPT Pro",
            vec![session(100.0), weekly(70.0)],
            AtLimitBehaviour::Credits { has_credits: true },
            None
        )
    );
}

#[test]
fn each_reached_type_maps_to_its_reason() {
    assert_eq!(
        read(include_str!(
            "../../../fixtures/codex/workspace-credits.json"
        )),
        plan(
            "team.user@example.com",
            "ChatGPT Team",
            vec![session(100.0)],
            AtLimitBehaviour::Credits { has_credits: false },
            Some(LimitStopReason::WorkspaceCredits)
        )
    );
    assert_eq!(
        read(include_str!("../../../fixtures/codex/spend-cap.json")),
        plan(
            "business.user@example.com",
            "ChatGPT Business",
            vec![session(80.0)],
            AtLimitBehaviour::Pauses,
            Some(LimitStopReason::WorkspaceSpendCap)
        )
    );
    assert_eq!(
        read(include_str!("../../../fixtures/codex/spend-control.json")),
        plan(
            "enterprise.user@example.com",
            "ChatGPT Enterprise",
            vec![session(50.0)],
            AtLimitBehaviour::Pauses,
            Some(LimitStopReason::WorkspaceSpendCap)
        )
    );
    assert_eq!(
        read(include_str!("../../../fixtures/codex/plan-window.json")),
        plan(
            "pro.user@example.com",
            "ChatGPT Pro",
            vec![session(100.0), weekly(55.0)],
            AtLimitBehaviour::Pauses,
            Some(LimitStopReason::PlanWindow)
        )
    );
}

/// The outcome of a `ChatGPT` `plan_type` account whose main snapshot is `snapshot`.
fn chatgpt(plan_type: &str, snapshot: &Value) -> ReadOutcome {
    outcome(answers(&json!({
        "accountRead": {
            "account": {"type": "chatgpt", "email": "x@example.com", "planType": plan_type},
            "requiresOpenaiAuth": true
        },
        "rateLimitsRead": {"rateLimits": snapshot}
    })))
}

fn limits_of(outcome: ReadOutcome) -> (AccountLimits, Option<LimitStopReason>) {
    match outcome {
        ReadOutcome::Usage { limits, reached } => (limits, reached),
        other => panic!("not usage: {other:?}"),
    }
}

#[test]
fn an_unknown_reached_value_is_a_plan_window_and_false_spend_control_is_nothing() {
    let (_, reached) = limits_of(chatgpt(
        "pro",
        &json!({"rateLimitReachedType": "brand_new_limit"}),
    ));
    assert_eq!(reached, Some(LimitStopReason::PlanWindow));
    let (_, reached) = limits_of(chatgpt("pro", &json!({"spendControlReached": false})));
    assert_eq!(reached, None);
    let (limits, reached) = limits_of(chatgpt("pro", &json!({})));
    assert_eq!((limits.windows, reached), (Vec::new(), None));
}

/// Spend control wins over a plain or unknown reached type, never over credits.
#[test]
fn spend_control_beats_a_plan_window_but_not_credits() {
    for kind in ["rate_limit_reached", "brand_new_limit"] {
        let snapshot = json!({"rateLimitReachedType": kind, "spendControlReached": true});
        let (_, reached) = limits_of(chatgpt("team", &snapshot));
        assert_eq!(reached, Some(LimitStopReason::WorkspaceSpendCap), "{kind}");
    }
    let snapshot = json!({
        "rateLimitReachedType": "workspace_member_credits_depleted",
        "spendControlReached": true
    });
    let (_, reached) = limits_of(chatgpt("team", &snapshot));
    assert_eq!(reached, Some(LimitStopReason::WorkspaceCredits));
}

#[test]
fn other_lengths_get_plain_labels_and_bad_ones_a_fallback() {
    let label = |mins: Value| {
        let (limits, _) = limits_of(chatgpt(
            "plus",
            &json!({"primary": {"usedPercent": 1, "windowDurationMins": mins}}),
        ));
        let window = limits.windows.into_iter().next().unwrap();
        (window.kind, window.label, window.duration_mins)
    };
    assert_eq!(
        label(json!(60)),
        (WindowKind::Other, "1-hour limit".to_owned(), Some(60))
    );
    assert_eq!(
        label(json!(1440)),
        (WindowKind::Other, "24-hour limit".to_owned(), Some(1440))
    );
    assert_eq!(
        label(json!(2880)),
        (WindowKind::Other, "2-day limit".to_owned(), Some(2880))
    );
    assert_eq!(
        label(json!(3000)),
        (WindowKind::Other, "50-hour limit".to_owned(), Some(3000))
    );
    assert_eq!(
        label(json!(90)),
        (WindowKind::Other, "90-minute limit".to_owned(), Some(90))
    );
    assert_eq!(
        label(json!(44_640)),
        (WindowKind::Other, "Monthly limit".to_owned(), Some(44_640))
    );
    assert_eq!(
        label(json!(44_700)),
        (WindowKind::Other, "745-hour limit".to_owned(), Some(44_700))
    );
    assert_eq!(
        label(json!(0)),
        (WindowKind::Other, "Usage limit".to_owned(), None)
    );
    assert_eq!(
        label(json!(-300)),
        (WindowKind::Other, "Usage limit".to_owned(), None)
    );
    let huge = i64::from(u32::MAX) * 1440 + 1440;
    let (_, label, mins) = label(json!(huge));
    assert_eq!((label.ends_with("-day limit"), mins), (true, None));
}

#[test]
fn percents_are_clamped_and_a_reset_that_overflows_is_dropped() {
    let (limits, _) = limits_of(chatgpt(
        "plus",
        &json!({
            "primary": {"usedPercent": -5, "windowDurationMins": 300, "resetsAt": i64::MAX},
            "secondary": {"usedPercent": 150, "windowDurationMins": 10080, "resetsAt": -1}
        }),
    ));
    let used: Vec<f64> = limits
        .windows
        .iter()
        .map(|window| window.used_percent.get())
        .collect();
    assert_eq!(used, [0.0, 100.0]);
    assert!(limits.windows[0].used_percent.get().is_sign_positive());
    assert_eq!(limits.windows[0].resets_at, None);
    assert_eq!(
        limits.windows[1].resets_at, None,
        "a reset at or before the epoch is dropped"
    );
}

#[test]
fn windows_sort_by_kind_not_position() {
    let (limits, _) = limits_of(chatgpt(
        "plus",
        &json!({
            "primary": {"usedPercent": 1, "windowDurationMins": 10080},
            "secondary": {"usedPercent": 2, "windowDurationMins": 300}
        }),
    ));
    let ids: Vec<&str> = limits
        .windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    assert_eq!(ids, ["secondary", "primary"]);
}

#[test]
fn a_single_view_of_another_limit_sets_no_windows() {
    let answers = answers(&json!({
        "accountRead": {
            "account": {"type": "chatgpt", "email": "x@example.com", "planType": "plus"},
            "requiresOpenaiAuth": true
        },
        "rateLimitsRead": {
            "rateLimits": {"limitId": "codex_spark", "planType": "pro", "primary": {"usedPercent": 90}},
            "rateLimitsByLimitId": {"codex_spark": {"limitId": "codex_spark", "primary": {"usedPercent": 90}}}
        }
    }));
    let (limits, reached) = limits_of(outcome(answers));
    assert_eq!(limits.windows, Vec::new());
    assert_eq!(limits.plan, Some(PlanLabel("ChatGPT Plus".to_owned())));
    assert_eq!(reached, None);
}

#[test]
fn unknown_plans_have_no_name_and_bad_emails_the_plan_ref() {
    for plan_type in [
        "ent26",
        "self_serve_business_usage_based",
        "unknown",
        "newplan",
    ] {
        let (limits, _) = limits_of(chatgpt(plan_type, &json!({})));
        assert_eq!(limits.plan, None, "{plan_type}");
    }
    let answers = answers(&json!({
        "accountRead": {
            "account": {"type": "chatgpt", "email": "not an email", "planType": "plus"},
            "requiresOpenaiAuth": true
        }
    }));
    let (limits, _) = limits_of(outcome(answers));
    assert_eq!(limits.account, AccountRef("plan".to_owned()));
}

#[test]
fn an_unknown_account_type_is_malformed_without_naming_it() {
    // Built at run time, so no file holds a token-shaped text.
    let secret_looking = format!("sk{}", "-secret-looking");
    let answers = answers(&json!({
        "accountRead": {"account": {"type": secret_looking}, "requiresOpenaiAuth": true}
    }));
    assert_eq!(
        outcome(answers),
        ReadOutcome::Failed(ReadFailure::Malformed("unknown account type".to_owned()))
    );
}

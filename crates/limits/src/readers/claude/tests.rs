//! The helper's line, from the fixtures and from hand-made edge cases, to the exact outcome.

mod prepare;

use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, BillingProvider, Finite,
    LimitSource, LimitWindow, PlanLabel, Timestamp, UnavailableReason, WindowKind,
};

use super::{Answer, answer, clean, looks_like_email};
use crate::{AccountRead, ReadFailure, ReadOutcome};

/// 2026-11-02T18:00:00Z.
const NOV_2_1800: i64 = 1_793_642_400_000;
/// 2026-11-02T15:30:00Z.
const NOV_2_1530: i64 = 1_793_633_400_000;
/// 2026-11-06T09:00:00Z.
const NOV_6_0900: i64 = 1_793_955_600_000;
/// A week, in minutes.
const WEEK: u32 = 10_080;

fn read(line: &str) -> Answer {
    answer(line.trim_end().as_bytes())
}

fn limits(line: &str) -> AccountLimits {
    match read(line) {
        Answer::Outcome(ReadOutcome::Usage {
            limits,
            reached: None,
            account_read: AccountRead::Read,
        }) => limits,
        other => panic!("not usage: {other:?}"),
    }
}

/// `window` counting `model`.
fn of(model: &str, window: LimitWindow) -> LimitWindow {
    LimitWindow {
        model: Some(model.to_owned()),
        ..window
    }
}

fn window(
    id: &str,
    kind: WindowKind,
    label: &str,
    used: f64,
    resets_at: Option<i64>,
    mins: u32,
) -> LimitWindow {
    LimitWindow {
        id: id.to_owned(),
        kind,
        label: label.to_owned(),
        used_percent: Finite::new(used).unwrap(),
        resets_at: resets_at.map(|ms| Timestamp::new(ms).unwrap()),
        duration_mins: Some(mins),
        warning: false,
        model: None,
    }
}

fn malformed(reason: &str) -> Answer {
    Answer::Outcome(ReadOutcome::Failed(ReadFailure::Malformed(
        reason.to_owned(),
    )))
}

fn fresh() -> LimitSource {
    LimitSource::Fresh {
        checked_at: Timestamp::ZERO,
    }
}

#[test]
fn max_has_its_windows_one_model_window_and_extra_usage_on() {
    let limits = limits(include_str!("../../../fixtures/claude/max.json"));
    assert_eq!(
        limits,
        AccountLimits {
            agent: AgentKind::Claude,
            account: AccountRef("max.user@example.com".to_owned()),
            plan: Some(PlanLabel("Claude Max".to_owned())),
            billing: Billing::Plan,
            windows: vec![
                window(
                    "five_hour",
                    WindowKind::Session,
                    "5-hour limit",
                    42.5,
                    Some(NOV_2_1800),
                    300
                ),
                window(
                    "seven_day",
                    WindowKind::Weekly,
                    "Weekly limit",
                    61.0,
                    Some(NOV_6_0900),
                    WEEK
                ),
                of(
                    "Fable",
                    window(
                        "seven_day_overage_included",
                        WindowKind::Model,
                        "Fable weekly limit",
                        12.0,
                        Some(NOV_6_0900),
                        WEEK
                    )
                ),
            ],
            at_limit: AtLimitBehaviour::ExtraUsage { enabled: true },
            source: fresh(),
        }
    );
}

#[test]
fn a_recorded_pro_line_reads_pro_with_its_session_and_weekly_windows() {
    let limits = limits(include_str!("../../../fixtures/recorded/claude/pro.jsonl"));
    assert_eq!(limits.account, AccountRef("test@example.com".to_owned()));
    assert_eq!(limits.plan, Some(PlanLabel("Claude Pro".to_owned())));
    assert_eq!(limits.billing, Billing::Plan);
    assert_eq!(limits.source, fresh());
    let shape: Vec<(&str, WindowKind)> = limits
        .windows
        .iter()
        .map(|window| (window.id.as_str(), window.kind))
        .collect();
    assert_eq!(
        shape,
        [
            ("five_hour", WindowKind::Session),
            ("seven_day", WindowKind::Weekly)
        ]
    );
}

#[test]
fn pro_reads_offsets_null_resets_and_extra_usage_off() {
    let limits = limits(include_str!("../../../fixtures/claude/pro.json"));
    assert_eq!(
        limits,
        AccountLimits {
            agent: AgentKind::Claude,
            account: AccountRef("pro.user@example.com".to_owned()),
            plan: Some(PlanLabel("Claude Pro".to_owned())),
            billing: Billing::Plan,
            windows: vec![
                window(
                    "five_hour",
                    WindowKind::Session,
                    "5-hour limit",
                    100.0,
                    Some(NOV_2_1530),
                    300
                ),
                window(
                    "seven_day",
                    WindowKind::Weekly,
                    "Weekly limit",
                    35.25,
                    None,
                    WEEK
                ),
                of(
                    "Sonnet",
                    window(
                        "seven_day_sonnet",
                        WindowKind::Model,
                        "Sonnet weekly limit",
                        20.0,
                        Some(NOV_6_0900),
                        WEEK
                    )
                ),
            ],
            at_limit: AtLimitBehaviour::ExtraUsage { enabled: false },
            source: fresh(),
        }
    );
}

#[test]
fn team_sorts_its_windows_and_numbers_two_model_windows_and_a_scoped_opus_replaces_the_fixed_one() {
    let limits = limits(include_str!("../../../fixtures/claude/team.json"));
    let at = Some(NOV_6_0900);
    assert_eq!(
        limits,
        AccountLimits {
            agent: AgentKind::Claude,
            account: AccountRef("plan".to_owned()),
            plan: Some(PlanLabel("Claude Team".to_owned())),
            billing: Billing::Plan,
            windows: vec![
                window(
                    "five_hour",
                    WindowKind::Session,
                    "5-hour limit",
                    5.0,
                    Some(NOV_2_1800),
                    300
                ),
                window(
                    "seven_day",
                    WindowKind::Weekly,
                    "Weekly limit",
                    10.0,
                    at,
                    WEEK
                ),
                of(
                    "Fable",
                    window(
                        "model_scoped:0",
                        WindowKind::Model,
                        "Fable weekly limit",
                        7.0,
                        at,
                        WEEK
                    )
                ),
                of(
                    "Opus",
                    window(
                        "model_scoped:1",
                        WindowKind::Model,
                        "Opus weekly limit",
                        8.5,
                        at,
                        WEEK
                    )
                ),
                window(
                    "seven_day_oauth_apps",
                    WindowKind::Other,
                    "Weekly limit for connected apps",
                    3.0,
                    at,
                    WEEK
                ),
            ],
            at_limit: AtLimitBehaviour::Pauses,
            source: fresh(),
        }
    );
}

fn pay_as_you_go(account: &str, via: BillingProvider) -> AccountLimits {
    AccountLimits {
        agent: AgentKind::Claude,
        account: AccountRef(account.to_owned()),
        plan: None,
        billing: Billing::PayAsYouGo { via },
        windows: Vec::new(),
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::NotApplicable,
    }
}

#[test]
fn api_key_bedrock_and_vertex_pay_as_they_go_with_fixed_refs() {
    assert_eq!(
        limits(include_str!("../../../fixtures/claude/api-key.json")),
        pay_as_you_go("api-key", BillingProvider::Anthropic)
    );
    assert_eq!(
        limits(include_str!("../../../fixtures/claude/bedrock.json")),
        pay_as_you_go("bedrock", BillingProvider::Bedrock)
    );
    assert_eq!(
        limits(include_str!("../../../fixtures/claude/vertex.json")),
        pay_as_you_go("vertex", BillingProvider::Vertex)
    );
}

#[test]
fn every_api_provider_maps_to_its_biller_and_ref() {
    for (provider, via, account) in [
        ("\"firstParty\"", BillingProvider::Anthropic, "api-key"),
        ("\"anthropicAws\"", BillingProvider::Bedrock, "bedrock"),
        (
            "\"anthropicGoogleCloud\"",
            BillingProvider::Vertex,
            "vertex",
        ),
        ("\"foundry\"", BillingProvider::Other, "api-key"),
        ("\"gateway\"", BillingProvider::Other, "api-key"),
        ("\"mantle\"", BillingProvider::Other, "api-key"),
        ("7", BillingProvider::Anthropic, "api-key"),
    ] {
        let line = format!(
            r#"{{"ok":true,"account":{{"apiProvider":{provider}}},"usage":{{"subscription_type":null,"rate_limits_available":false,"rate_limits":null}}}}"#
        );
        assert_eq!(limits(&line), pay_as_you_go(account, via), "{provider}");
    }
}

#[test]
fn a_pay_as_you_go_email_is_the_account() {
    let line = r#"{"ok":true,"account":{"email":"payg@example.com","apiProvider":"bedrock"},"usage":{"subscription_type":null,"rate_limits_available":false}}"#;
    assert_eq!(
        limits(line),
        pay_as_you_go("payg@example.com", BillingProvider::Bedrock)
    );
}

#[test]
fn missing_scope_is_a_plan_with_no_windows() {
    assert_eq!(
        limits(include_str!("../../../fixtures/claude/missing-scope.json")),
        AccountLimits {
            agent: AgentKind::Claude,
            account: AccountRef("scope.user@example.com".to_owned()),
            plan: Some(PlanLabel("Claude Pro".to_owned())),
            billing: Billing::Plan,
            windows: Vec::new(),
            at_limit: AtLimitBehaviour::Pauses,
            source: LimitSource::Unavailable {
                reason: UnavailableReason::MissingScope
            },
        }
    );
}

#[test]
fn the_two_failure_lines_are_method_missing_and_a_helper_failure() {
    assert_eq!(
        read(include_str!("../../../fixtures/claude/method-missing.json")),
        Answer::Outcome(ReadOutcome::MethodMissing)
    );
    assert_eq!(
        read(include_str!("../../../fixtures/claude/init-failed.json")),
        Answer::HelperFailed
    );
}

#[test]
fn lines_of_neither_form_are_malformed_without_quoting_them() {
    assert_eq!(read("not json at all"), malformed("the line is not JSON"));
    assert_eq!(read(""), malformed("the line is not JSON"));
    assert_eq!(read("[1,2]"), malformed("the line is not a JSON object"));
    assert_eq!(
        read(r#"{"usage":{}}"#),
        malformed("the line has no ok field")
    );
    assert_eq!(
        read(r#"{"ok":"true"}"#),
        malformed("the line has no ok field")
    );
    assert_eq!(
        read(r#"{"ok":false,"stage":"init","message":"x"}"#),
        malformed("an ok:false line has no missing field")
    );
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    assert_eq!(read(&deep), malformed("the line is not JSON"));
}

#[test]
fn a_usage_that_does_not_fit_the_schema_is_method_missing() {
    for usage in [
        r#""usage":null"#,
        r#""usage":[]"#,
        r#""usage":{}"#,
        r#""usage":{"subscription_type":"max"}"#,
        r#""usage":{"rate_limits_available":"yes"}"#,
        r#""usage":{"rate_limits_available":true,"subscription_type":5}"#,
        r#""usage":{"rate_limits_available":true,"rate_limits":[]}"#,
    ] {
        let line = format!(r#"{{"ok":true,"account":null,{usage}}}"#);
        assert_eq!(
            read(&line),
            Answer::Outcome(ReadOutcome::MethodMissing),
            "{usage}"
        );
    }
    assert_eq!(
        read(r#"{"ok":true}"#),
        Answer::Outcome(ReadOutcome::MethodMissing)
    );
}

fn windows_of(rate_limits: &str) -> Vec<LimitWindow> {
    let line = format!(
        r#"{{"ok":true,"account":null,"unknown":1,"usage":{{"subscription_type":"enterprise","rate_limits_available":true,"rate_limits":{rate_limits}}}}}"#
    );
    limits(&line).windows
}

#[test]
fn a_bad_window_is_dropped_alone() {
    let windows = windows_of(
        r#"{"five_hour":{"utilization":"x","resets_at":"2026-11-02T18:00:00Z"},
            "seven_day":{"utilization":150,"resets_at":"not a time","extra":true},
            "seven_day_opus":{"resets_at":"2026-11-02T18:00:00Z"},
            "seven_day_sonnet":"broken",
            "seven_day_oauth_apps":{"utilization":-3,"resets_at":7},
            "model_scoped":[{"display_name":5,"utilization":1},{"display_name":"  ","utilization":1},
                            {"display_name":"Big\u0007Model","utilization":2}]}"#,
    );
    assert_eq!(
        windows,
        [
            window(
                "seven_day",
                WindowKind::Weekly,
                "Weekly limit",
                100.0,
                None,
                WEEK
            ),
            of(
                "Big Model",
                window(
                    "model_scoped:2",
                    WindowKind::Model,
                    "Big Model weekly limit",
                    2.0,
                    None,
                    WEEK
                )
            ),
            window(
                "seven_day_oauth_apps",
                WindowKind::Other,
                "Weekly limit for connected apps",
                0.0,
                None,
                WEEK
            ),
        ]
    );
    assert!(
        windows
            .iter()
            .all(|window| window.used_percent.get().is_sign_positive())
    );
}

#[test]
fn a_fixed_model_window_gives_way_only_to_a_scoped_row_of_the_same_model() {
    let ids_and_labels = |rate_limits: &str| -> Vec<(String, String)> {
        windows_of(rate_limits)
            .into_iter()
            .map(|window| (window.id, window.label))
            .collect()
    };
    let pair = |id: &str, label: &str| (id.to_owned(), label.to_owned());
    // The same model, in another case: the scoped row stays, the fixed one goes. Sonnet, which
    // no scoped row names, stays.
    assert_eq!(
        ids_and_labels(
            r#"{"seven_day_opus":{"utilization":50},"seven_day_sonnet":{"utilization":20},
                "model_scoped":[{"display_name":"opus","utilization":8}]}"#
        ),
        [
            pair("seven_day_sonnet", "Sonnet weekly limit"),
            pair("seven_day_overage_included", "opus weekly limit"),
        ]
    );
    // A scoped entry with no number is no row, so it replaces nothing.
    assert_eq!(
        ids_and_labels(
            r#"{"seven_day_opus":{"utilization":50},
                "model_scoped":[{"display_name":"Opus","utilization":null}]}"#
        ),
        [pair("seven_day_opus", "Opus weekly limit")]
    );
    // A model the fixed windows never name gets its row from the server's name, beside them.
    assert_eq!(
        ids_and_labels(
            r#"{"seven_day_opus":{"utilization":5},
                "model_scoped":[{"display_name":"Fable","utilization":1},
                                {"display_name":"Lyric 2","utilization":2}]}"#
        ),
        [
            pair("seven_day_opus", "Opus weekly limit"),
            pair("model_scoped:0", "Fable weekly limit"),
            pair("model_scoped:1", "Lyric 2 weekly limit"),
        ]
    );
}

#[test]
fn a_model_window_carries_the_bare_model_name_and_others_none() {
    let models: Vec<Option<String>> = windows_of(
        r#"{"five_hour":{"utilization":1},"seven_day_sonnet":{"utilization":2},
            "model_scoped":[{"display_name":"\u202e Fable\u200b ","utilization":3}]}"#,
    )
    .into_iter()
    .map(|window| window.model)
    .collect();
    assert_eq!(
        models,
        [None, Some("Sonnet".to_owned()), Some("Fable".to_owned())]
    );
}

#[test]
fn windows_are_capped_at_sixteen_and_labels_at_sixty_four_chars() {
    let entry = r#"{"display_name":"ééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééé","utilization":1}"#;
    let models = vec![entry; 40].join(",");
    let windows = windows_of(&format!(
        r#"{{"five_hour":{{"utilization":1}},"seven_day_oauth_apps":{{"utilization":1}},"model_scoped":[{models}]}}"#
    ));
    assert_eq!(windows.len(), 16);
    assert_eq!(windows.first().unwrap().id, "five_hour");
    assert!(
        windows
            .iter()
            .all(|window| window.kind != WindowKind::Other)
    );
    let label = &windows[1].label;
    assert_eq!(label.chars().count(), 64);
    assert!(label.chars().all(|c| c == 'é'));
}

#[test]
fn rate_limits_null_with_limits_available_reads_fresh_with_no_windows() {
    assert_eq!(windows_of("null"), []);
    let line = r#"{"ok":true,"account":null,"usage":{"subscription_type":"enterprise","rate_limits_available":true,"rate_limits":{"extra_usage":{"is_enabled":"yes"}}}}"#;
    let limits = limits(line);
    assert_eq!(limits.plan, Some(PlanLabel("Claude Enterprise".to_owned())));
    assert_eq!(limits.at_limit, AtLimitBehaviour::Pauses);
    assert_eq!(limits.source, fresh());
}

#[test]
fn an_unknown_plan_has_no_name() {
    let line = r#"{"ok":true,"account":{"email":"x@example.com"},"usage":{"subscription_type":"ultra","rate_limits_available":true,"rate_limits":{}}}"#;
    let limits = limits(line);
    assert_eq!(limits.plan, None);
    assert_eq!(limits.billing, Billing::Plan);
}

#[test]
fn only_an_email_shaped_account_is_kept() {
    // Built at run time, so no file holds a token-shaped text.
    let key_shaped = format!("sk{}", "-ant-api03-0123456789abcdef");
    assert!(looks_like_email("a@b"));
    assert!(looks_like_email("lawyer@example.com"));
    for text in [
        "",
        "ab",
        "@example.com",
        "lawyer@",
        "a@b@c",
        "no at sign",
        "law yer@example.com",
        "lawyer@example.com\n",
        "lawyer\u{0}@example.com",
        key_shaped.as_str(),
    ] {
        assert!(!looks_like_email(text), "{text:?}");
    }
    assert!(looks_like_email(&format!(
        "{}@example.com",
        "a".repeat(242)
    )));
    assert!(!looks_like_email(&format!(
        "{}@example.com",
        "a".repeat(243)
    )));
    let token = format!("sk{}", "-ant-secret-token");
    let line = format!(
        r#"{{"ok":true,"account":{{"email":"{token}"}},"usage":{{"subscription_type":"max","rate_limits_available":false}}}}"#
    );
    assert_eq!(limits(&line).account, AccountRef("plan".to_owned()));
}

#[test]
fn format_and_bidi_characters_fail_an_email_and_leave_a_label() {
    for c in [
        '\u{200B}', '\u{200F}', '\u{202A}', '\u{202E}', '\u{2060}', '\u{2064}', '\u{2066}',
        '\u{2069}', '\u{FEFF}',
    ] {
        assert!(
            !looks_like_email(&format!("law{c}yer@example.com")),
            "{c:?}"
        );
        assert_eq!(clean(&format!("{c}Fa{c}ble{c}")), "Fable", "{c:?}");
    }
    assert!(looks_like_email("l\u{00E9}a@example.com"));
    assert_eq!(clean("\u{202E}Opus\u{0007}4"), "Opus 4");
    let windows =
        windows_of(r#"{"model_scoped":[{"display_name":"\u202eFable\u200b","utilization":1}]}"#);
    assert_eq!(windows[0].label, "Fable weekly limit");
}

//! A model's bucket in `rateLimitsByLimitId`: its windows as `Model` rows named by the limit,
//! after Codex's own rows.

use clauseo_protocol::{LimitWindow, WindowKind};
use serde_json::{Value, json};

use super::{FIVE_HOUR_RESET, WEEK_RESET, answers, limits_of, outcome, session, weekly, window};

/// One row of `plus.json`'s Spark bucket.
fn spark(position: &str, label: &str, used: f64, reset: i64, mins: u32) -> LimitWindow {
    LimitWindow {
        model: Some("Spark".to_owned()),
        ..window(
            &format!("codex_spark:{position}"),
            WindowKind::Model,
            label,
            used,
            Some(reset),
            Some(mins),
        )
    }
}

/// The windows `plus.json` maps to: Codex's own two, then the Spark bucket's two.
pub(super) fn plus_windows() -> Vec<LimitWindow> {
    vec![
        session(25.0),
        weekly(40.0),
        spark("primary", "Spark 5-hour limit", 90.0, FIVE_HOUR_RESET, 300),
        spark("secondary", "Spark weekly limit", 95.0, WEEK_RESET, 10_080),
    ]
}

/// The windows a Plus read maps to when `rateLimitsByLimitId` is `by_id`.
fn windows_of(by_id: &Value) -> Vec<LimitWindow> {
    let (limits, _) = limits_of(outcome(answers(&json!({
        "accountRead": {
            "account": {"type": "chatgpt", "email": "x@example.com", "planType": "plus"},
            "requiresOpenaiAuth": true
        },
        "rateLimitsRead": {"rateLimits": {"limitId": "codex"}, "rateLimitsByLimitId": by_id}
    }))));
    limits.windows
}

/// Each window's `(id, label, model)`.
fn named(windows: &[LimitWindow]) -> Vec<(&str, &str, Option<&str>)> {
    windows
        .iter()
        .map(|window| {
            (
                window.id.as_str(),
                window.label.as_str(),
                window.model.as_deref(),
            )
        })
        .collect()
}

#[test]
fn only_a_named_bucket_other_than_codex_gives_model_rows() {
    let five_hour = json!({"usedPercent": 10, "windowDurationMins": 300});
    let windows = windows_of(&json!({
        "codex": {"limitId": "codex", "limitName": "Codex", "primary": five_hour},
        "a_blank": {"limitName": " \u{200b}\t", "primary": five_hour},
        "b_none": {"primary": five_hour},
        "c_bent": {"limitName": " Spark\u{7}Fast ", "primary": five_hour},
        "d_number": {"limitName": "Mini", "primary": {"usedPercent": 150}},
    }));
    assert_eq!(
        named(&windows),
        [
            ("primary", "5-hour limit", None),
            (
                "c_bent:primary",
                "Spark Fast 5-hour limit",
                Some("Spark Fast")
            ),
            ("d_number:primary", "Mini usage limit", Some("Mini")),
        ]
    );
    assert_eq!(windows[1].kind, WindowKind::Model);
    assert_eq!(windows[2].used_percent.get(), 100.0);
}

#[test]
fn a_model_row_takes_its_length_from_codex_rules_and_its_name_is_capped() {
    let long = "x".repeat(70);
    let windows = windows_of(&json!({
        "m": {
            "limitName": "Spark",
            "primary": {"usedPercent": 1, "windowDurationMins": 43_200},
            "secondary": {"usedPercent": 2, "windowDurationMins": 4_320}
        },
        "n": {"limitName": long, "secondary": {"usedPercent": 3, "windowDurationMins": 10_080}},
    }));
    let capped = "x".repeat(64);
    assert_eq!(
        named(&windows),
        [
            ("m:primary", "Spark monthly limit", Some("Spark")),
            ("m:secondary", "Spark 3-day limit", Some("Spark")),
            ("n:secondary", capped.as_str(), Some(capped.as_str())),
        ]
    );
    assert_eq!(windows[0].duration_mins, Some(43_200));
}

#[test]
fn a_read_keeps_sixteen_windows_codex_rows_first() {
    let mut by_id = json!({
        "codex": {
            "primary": {"usedPercent": 1, "windowDurationMins": 300},
            "secondary": {"usedPercent": 1, "windowDurationMins": 10_080}
        }
    });
    for n in 0..10 {
        by_id[format!("m{n:02}")] = json!({
            "limitName": format!("Model {n}"),
            "primary": {"usedPercent": 1, "windowDurationMins": 300},
            "secondary": {"usedPercent": 1, "windowDurationMins": 10_080}
        });
    }
    let windows = windows_of(&by_id);
    let ids: Vec<&str> = windows.iter().map(|window| window.id.as_str()).collect();
    assert_eq!(ids.len(), 16);
    assert_eq!(
        ids[..4],
        ["primary", "secondary", "m00:primary", "m00:secondary"]
    );
    assert_eq!(ids[15], "m06:secondary");
}

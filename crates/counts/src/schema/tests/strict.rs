//! The strict reads: each payload refuses an unknown field, an unknown name, a number over its
//! cap, an id, day or version in any other form, and `StrictErrorCode` refuses what
//! `ErrorCode::from_str` refuses and `unknown` too.

use clauseo_protocol::ErrorCode;
use serde_json::{Value, json};

use crate::schema::{
    Batch, Day, Invalid, StrictErrorCode, TallyReport, Version, parse_batch, parse_tally,
};

fn day_json() -> Value {
    json!({
        "schema": 1,
        "install": "8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40",
        "day": "2026-11-01",
        "version": "3.0.4",
        "channel": "stable",
        "os": "macos",
        "os_major": 15,
        "arch": "aarch64",
        "language": "en",
        "cells": {
            "app_opened": [{ "launch": "cold", "n": 2 }],
            "time_open": [{ "bucket": "lt_3h", "n": 1 }],
            "question_sent": [{ "agent": "claude", "delivery": "now", "n": 14 }],
            "matter_opened": [{ "kind": "folder", "papers": "11_50", "protection": "not_protected", "n": 1 }],
            "setting_changed": [
                { "setting": "text_size", "value": 18, "n": 1 },
                { "setting": "theme", "value": "dark", "n": 1 }
            ],
            "error_shown": [{ "code": "agent.exited", "n": 1 }]
        }
    })
}

fn today() -> Day {
    Day::from_ymd(2026, 11, 2).unwrap()
}

fn batch(day: Value) -> Result<Batch, Invalid> {
    parse_batch(
        &serde_json::to_vec(&json!({ "days": vec![day] })).unwrap(),
        today(),
    )
}

/// `day_json()` with `edit` applied at `pointer`.
fn edited(pointer: &str, edit: Value) -> Value {
    let mut day = day_json();
    *day.pointer_mut(pointer).unwrap() = edit;
    day
}

#[test]
fn the_example_day_is_taken_and_written_back_the_same() {
    let batch = batch(day_json()).unwrap();
    let back = serde_json::to_value(&batch.days[0]).unwrap();
    assert_eq!(back, day_json());
}

#[test]
fn an_unknown_field_anywhere_is_refused() {
    let mut extra_top = day_json();
    extra_top["matter"] = json!("Mehra divorce");
    let mut extra_cell = day_json();
    extra_cell["cells"]["app_opened"][0]["path"] = json!("/Users/priya");
    let mut extra_event = day_json();
    extra_event["cells"]["crash"] = json!([]);
    let mut fleet = day_json();
    fleet["managed"] = json!(true);
    for day in [extra_top, extra_cell, extra_event, fleet] {
        assert_eq!(batch(day).unwrap_err(), Invalid::Schema);
    }
    let mut extra_batch = json!({ "days": [day_json()] });
    extra_batch["note"] = json!("x");
    assert!(parse_batch(&serde_json::to_vec(&extra_batch).unwrap(), today()).is_err());
}

#[test]
fn an_unknown_name_or_a_number_past_its_cap_is_refused() {
    let cases = [
        ("/cells/app_opened/0/launch", json!("warm")),
        ("/cells/app_opened/0/n", json!(10_000)),
        ("/cells/app_opened/0/n", json!(0)),
        ("/cells/app_opened/0/n", json!(1.5)),
        ("/cells/app_opened/0/n", json!("2")),
        ("/cells/setting_changed/0/value", json!(21)),
        ("/cells/setting_changed/1/value", json!("blue")),
        ("/cells/setting_changed/1/setting", json!("matters_folder")),
        ("/cells/error_shown/0/code", json!("unknown")),
        ("/cells/error_shown/0/code", json!("agent.made_up")),
        ("/os_major", json!(41)),
        ("/schema", json!(2)),
        ("/channel", json!("nightly")),
        ("/language", json!("fr")),
    ];
    for (pointer, value) in cases {
        assert_eq!(
            batch(edited(pointer, value.clone())).unwrap_err(),
            Invalid::Schema,
            "{pointer} = {value}"
        );
    }
}

#[test]
fn an_id_day_or_version_in_another_form_is_refused() {
    let cases = [
        ("/install", "8F9A2C1E-4B7D-4E0A-9C3F-2D6B1A8E5F40"),
        ("/install", "{8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40}"),
        ("/install", "8f9a2c1e4b7d4e0a9c3f2d6b1a8e5f40"),
        ("/install", "8f9a2c1e-4b7d-1e0a-9c3f-2d6b1a8e5f40"),
        ("/install", "urn:uuid:8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40"),
        ("/day", "2026-11-1"),
        ("/day", "2026-11-01T00:00:00Z"),
        ("/version", "3.0.4-beta.1"),
        ("/version", "3.0"),
        ("/version", "03.0.4"),
        ("/version", "3.0.4 "),
    ];
    for (pointer, text) in cases {
        assert_eq!(
            batch(edited(pointer, json!(text))).unwrap_err(),
            Invalid::Schema,
            "{pointer} = {text}"
        );
    }
    assert_eq!(
        "999.0.12".parse::<Version>().map(|v| v.to_string()),
        Ok("999.0.12".to_owned())
    );
    assert!("1000.0.0".parse::<Version>().is_err());
}

#[test]
fn strict_error_code_refuses_unknown_and_what_from_str_refuses() {
    for code in ErrorCode::ALL {
        let text = json!(code.as_str());
        let read = serde_json::from_value::<StrictErrorCode>(text);
        if code == ErrorCode::Unknown {
            assert!(read.is_err());
        } else {
            assert_eq!(read.unwrap().get(), code);
        }
    }
    for text in [
        "unknown",
        "agent.made_up",
        "",
        "AGENT.EXITED",
        " agent.exited",
    ] {
        assert!(text.parse::<ErrorCode>().is_err() || text == "unknown");
        assert!(
            serde_json::from_value::<StrictErrorCode>(json!(text)).is_err(),
            "{text}"
        );
    }
    assert!(serde_json::from_value::<StrictErrorCode>(json!(3)).is_err());
}

#[test]
fn the_rules_beyond_the_types_hold() {
    let cases = [
        (edited("/day", json!("2026-11-02")), Invalid::OutOfWindow),
        (edited("/day", json!("2026-10-18")), Invalid::OutOfWindow),
        (edited("/os_major", json!(11)), Invalid::Platform),
        (edited("/cells", json!({})), Invalid::Empty),
        (
            edited(
                "/cells/time_open",
                json!([{ "bucket": "lt_3h", "n": 1 }, { "bucket": "lt_1h", "n": 1 }]),
            ),
            Invalid::Cells,
        ),
        (
            edited(
                "/cells/app_opened",
                json!([{ "launch": "cold", "n": 1 }, { "launch": "cold", "n": 1 }]),
            ),
            Invalid::Cells,
        ),
    ];
    for (day, want) in cases {
        assert_eq!(batch(day).unwrap_err(), want);
    }
    assert!(batch(edited("/day", json!("2026-10-19"))).is_ok());
    // macOS 27, current when the cap was raised, to the cap itself.
    assert!(batch(edited("/os_major", json!(27))).is_ok());
    assert!(batch(edited("/os_major", json!(40))).is_ok());
    let mut linux = day_json();
    linux["os"] = json!("linux");
    assert_eq!(batch(linux.clone()).unwrap_err(), Invalid::Platform);
    linux.as_object_mut().unwrap().remove("os_major");
    assert!(batch(linux).is_ok());

    let eight: Vec<Value> = (1..=8)
        .map(|age| edited("/day", json!(format!("2026-10-{:02}", 32 - age))))
        .collect();
    let body = serde_json::to_vec(&json!({ "days": eight })).unwrap();
    assert_eq!(parse_batch(&body, today()).unwrap_err(), Invalid::DayCount);
    let twice = serde_json::to_vec(&json!({ "days": [day_json(), day_json()] })).unwrap();
    assert_eq!(
        parse_batch(&twice, today()).unwrap_err(),
        Invalid::RepeatedDay
    );
    assert_eq!(
        parse_batch(&vec![b' '; 64_001], today()).unwrap_err(),
        Invalid::TooLarge
    );
}

#[test]
fn a_tally_is_taken_once_its_week_closes_and_refuses_an_install() {
    let tally = json!({
        "schema": 1,
        "tally": "1c0e7d52-9a43-4f1b-8e6d-3b2a9f7c4d18",
        "week": "2026-W44",
        "version": "3.0.4",
        "channel": "stable",
        "os": "windows",
        "os_major": 11,
        "arch": "x86_64",
        "install_type": "per_machine",
        "managed": true,
        "cells": {
            "crash": [{ "process": "webview", "kind": "renderer_gone", "after_launch": false, "n": 1 }],
            "active_week": [{ "n": 1 }]
        }
    });
    let this_week = today().week();
    let body = serde_json::to_vec(&tally).unwrap();
    let read: TallyReport = parse_tally(&body, this_week).unwrap();
    assert_eq!(serde_json::to_value(&read).unwrap(), tally);

    let mut with_install = tally.clone();
    with_install["install"] = json!("8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40");
    assert_eq!(
        parse_tally(&serde_json::to_vec(&with_install).unwrap(), this_week).unwrap_err(),
        Invalid::Schema
    );
    let mut open_week = tally.clone();
    open_week["week"] = json!(this_week.to_string());
    assert_eq!(
        parse_tally(&serde_json::to_vec(&open_week).unwrap(), this_week).unwrap_err(),
        Invalid::OutOfWindow
    );
    let mut old = tally;
    old["week"] = json!("2026-W40");
    assert_eq!(
        parse_tally(&serde_json::to_vec(&old).unwrap(), this_week).unwrap_err(),
        Invalid::OutOfWindow
    );
}

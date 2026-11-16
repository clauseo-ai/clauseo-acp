//! The rows made from the crate's own example day and tally (`crates/counts/src/schema/tests/
//! strict.rs`), and the canonical form a reordered, spaced-out body is stored in.

use clauseo_counts::schema::{Day, InstallId, TallyId, Time};
use serde_json::{Value, json};

use super::*;

/// `strict.rs`'s `day_json()`.
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

/// `strict.rs`'s tally.
fn tally_json() -> Value {
    json!({
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
    })
}

/// Noon on 2026-11-02, the day `strict.rs` calls today.
fn now() -> Time {
    Day::from_ymd(2026, 11, 2).unwrap().start().plus(12 * 3_600)
}

fn text(value: &str) -> Param {
    Param::Text(value.to_owned())
}

fn counts(body: &[u8]) -> Result<Accepted, Refusal> {
    accept(Route::Counts, body, now())
}

/// The crate's own order: events in `DayCells`' field order, a cell's fields sorted with `n`.
const CANONICAL_DAY_CELLS: &str = concat!(
    r#"{"app_opened":[{"launch":"cold","n":2}],"#,
    r#""time_open":[{"bucket":"lt_3h","n":1}],"#,
    r#""matter_opened":[{"kind":"folder","n":1,"papers":"11_50","protection":"not_protected"}],"#,
    r#""question_sent":[{"agent":"claude","delivery":"now","n":14}],"#,
    r#""error_shown":[{"code":"agent.exited","n":1}],"#,
    r#""setting_changed":[{"n":1,"setting":"text_size","value":18},"#,
    r#"{"n":1,"setting":"theme","value":"dark"}]}"#,
);

#[test]
fn the_example_day_becomes_one_row_of_its_fields_and_canonical_cells() {
    let body = serde_json::to_vec(&json!({ "days": [day_json()] })).unwrap();
    let accepted = counts(&body).unwrap();
    assert_eq!(accepted.insert, INSERT_REPORT);
    assert_eq!(
        accepted.rows,
        vec![vec![
            text("8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40"),
            text("2026-11-01"),
            text("3.0.4"),
            text("stable"),
            text("macos"),
            Param::Int(15),
            text("aarch64"),
            text("en"),
            text(CANONICAL_DAY_CELLS),
        ]]
    );
}

#[test]
fn spaces_and_reordered_keys_are_stored_in_the_canonical_form() {
    let messy = r#"{ "days" : [ {
        "cells": { "error_shown": [ { "n": 1, "code": "agent.exited" } ],
                   "setting_changed": [ { "value": 18, "n": 1, "setting": "text_size" },
                                        { "value": "dark", "setting": "theme", "n": 1 } ],
                   "matter_opened": [ { "protection": "not_protected", "papers": "11_50", "n": 1, "kind": "folder" } ],
                   "question_sent": [ { "n": 14, "delivery": "now", "agent": "claude" } ],
                   "time_open": [ { "n": 1, "bucket": "lt_3h" } ],
                   "app_opened": [ { "n": 2, "launch": "cold" } ] },
        "language" : "en", "arch": "aarch64", "os_major": 15, "os": "macos",
        "channel": "stable", "version": "3.0.4", "day": "2026-11-01",
        "install": "8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40", "schema": 1
    } ] }"#;
    let tidy = serde_json::to_vec(&json!({ "days": [day_json()] })).unwrap();
    assert_eq!(counts(messy.as_bytes()), counts(&tidy));
}

#[test]
fn linux_has_no_os_major_and_the_column_is_null() {
    let mut day = day_json();
    day["os"] = json!("linux");
    day.as_object_mut().unwrap().remove("os_major");
    let body = serde_json::to_vec(&json!({ "days": [day] })).unwrap();
    assert_eq!(counts(&body).unwrap().rows[0][5], Param::Null);
}

#[test]
fn seven_days_are_seven_rows_and_anything_the_crate_refuses_is_bad() {
    let days: Vec<Value> = (1..=7)
        .map(|age| {
            let mut day = day_json();
            day["day"] = json!(now().day().unwrap().days_before(age).unwrap().to_string());
            day
        })
        .collect();
    let body = serde_json::to_vec(&json!({ "days": days })).unwrap();
    assert_eq!(counts(&body).unwrap().rows.len(), 7);

    let mut extra = day_json();
    extra["note"] = json!("x");
    let mut today = day_json();
    today["day"] = json!("2026-11-02");
    let mut code = day_json();
    code["cells"]["error_shown"][0]["code"] = json!("agent.made_up");
    for day in [extra, today, code] {
        let body = serde_json::to_vec(&json!({ "days": [day] })).unwrap();
        assert_eq!(counts(&body), Err(Refusal::Bad));
    }
    assert_eq!(counts(b"not json"), Err(Refusal::Bad));
}

#[test]
fn a_tally_becomes_one_row_with_managed_as_an_integer() {
    let body = serde_json::to_vec(&tally_json()).unwrap();
    let accepted = accept(Route::Tally, &body, now()).unwrap();
    assert_eq!(accepted.insert, INSERT_TALLY);
    assert_eq!(
        accepted.rows,
        vec![vec![
            text("1c0e7d52-9a43-4f1b-8e6d-3b2a9f7c4d18"),
            text("2026-W44"),
            text("3.0.4"),
            text("stable"),
            text("windows"),
            Param::Int(11),
            text("x86_64"),
            text("per_machine"),
            Param::Int(1),
            text(
                r#"{"crash":[{"after_launch":false,"kind":"renderer_gone","n":1,"process":"webview"}],"active_week":[{"n":1}]}"#
            ),
        ]]
    );
    let mut with_install = tally_json();
    with_install["install"] = json!("8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40");
    let body = serde_json::to_vec(&with_install).unwrap();
    assert_eq!(accept(Route::Tally, &body, now()), Err(Refusal::Bad));
    // A day report is not a tally, nor a tally a batch.
    let day = serde_json::to_vec(&json!({ "days": [day_json()] })).unwrap();
    assert_eq!(accept(Route::Tally, &day, now()), Err(Refusal::Bad));
    assert_eq!(
        counts(&serde_json::to_vec(&tally_json()).unwrap()),
        Err(Refusal::Bad)
    );
}

/// The live check's canary ids, as `scripts/deploy.mjs` holds them.
fn canary(name: &str) -> &'static str {
    let script = include_str!("../../scripts/deploy.mjs");
    let start = script.find(&format!("const {name} = \"")).unwrap() + name.len() + 10;
    &script[start..start + 36]
}

#[test]
fn the_canary_ids_are_ids_the_crate_reads() {
    let install = canary("CANARY_INSTALL");
    let tally = canary("CANARY_TALLY");
    assert_eq!(install, "c0ffee00-0000-4000-8000-000000000001");
    assert_eq!(tally, "c0ffee00-0000-4000-8000-000000000002");
    assert_eq!(install.parse::<InstallId>().unwrap().to_string(), install);
    assert_eq!(tally.parse::<TallyId>().unwrap().to_string(), tally);
    let mut day = day_json();
    day["install"] = json!(install);
    assert!(counts(&serde_json::to_vec(&json!({ "days": [day] })).unwrap()).is_ok());
}

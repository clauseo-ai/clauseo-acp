//! The object and row made from the crate's own example reports
//! (`crates/counts/src/schema/tests/crash.rs`), the canonical form, the signatures and the hour.

use std::fmt::Write as _;

use clauseo_counts::schema::Day;
use serde_json::{Value, json};

use super::*;

/// Wednesday 2026-11-11, 14:25 UTC, in ISO week 2026-W46.
fn now() -> Time {
    Day::from_ymd(2026, 11, 11)
        .unwrap()
        .start()
        .plus(14 * 3_600 + 25 * 60)
}

/// `crash.rs`'s `panic_json()`.
fn panic_json() -> Value {
    json!({
        "schema": 1,
        "report": "3d1f8a62-5c4b-4e7a-b9d0-1f2e3a4b5c6d",
        "week": "2026-W45",
        "version": "3.0.4",
        "channel": "stable",
        "os": "windows",
        "os_major": 11,
        "arch": "x86_64",
        "process": "host",
        "kind": "panic",
        "area": "store",
        "after_launch": true,
        "location": { "file": "crates/store/src/writer.rs", "line": 212 }
    })
}

fn upload(report: &Value) -> Result<Upload, Refusal> {
    accept(&serde_json::to_vec(report).unwrap(), now())
}

fn text(value: &str) -> Param {
    Param::Text(value.to_owned())
}

#[test]
fn the_example_panic_becomes_its_object_and_row() {
    let made = upload(&panic_json()).unwrap();
    assert_eq!(
        made.key,
        "reports/3d1f8a62-5c4b-4e7a-b9d0-1f2e3a4b5c6d.json"
    );
    assert_eq!(
        made.row,
        [
            "3d1f8a62-5c4b-4e7a-b9d0-1f2e3a4b5c6d",
            "2026-W45",
            "3.0.4",
            "windows",
            "x86_64",
            "panic",
            "store",
            "crates/store/src/writer.rs:212",
        ]
        .map(text)
    );
    let stored: Value = serde_json::from_slice(&made.body).unwrap();
    assert_eq!(stored, panic_json());
    assert!(!made.body.contains(&b' '), "no whitespace");
}

#[test]
fn spaces_and_reordered_keys_are_stored_in_the_canonical_form() {
    let tidy = upload(&panic_json()).unwrap();
    let object = panic_json();
    let mut pairs: Vec<(String, Value)> = object.as_object().unwrap().clone().into_iter().collect();
    pairs.reverse();
    let mut messy = String::from("{\n");
    for (n, (key, value)) in pairs.iter().enumerate() {
        let comma = if n + 1 < pairs.len() { "," } else { "" };
        writeln!(messy, "   {key:?} :  {value}{comma}").unwrap();
    }
    messy.push('}');
    let made = accept(messy.as_bytes(), now()).unwrap();
    assert_eq!(made.body, tidy.body);
    assert_eq!(made.row, tidy.row);
}

#[test]
fn each_kind_has_its_signature() {
    let mut signal = panic_json();
    let object = signal.as_object_mut().unwrap();
    object.remove("location");
    object.insert("kind".to_owned(), json!("signal"));
    object.insert("area".to_owned(), json!("other"));
    object.insert("code".to_owned(), json!(3_221_225_477_u32));
    assert_eq!(upload(&signal).unwrap().row[7], text("code:3221225477"));

    let renderer = json!({
        "schema": 1,
        "report": "3d1f8a62-5c4b-4e7a-b9d0-1f2e3a4b5c6d",
        "week": "2026-W44",
        "version": "3.0.4",
        "channel": "beta",
        "os": "linux",
        "arch": "aarch64",
        "process": "webview",
        "kind": "renderer_gone",
        "area": "webview",
        "after_launch": false,
        "reason": "out_of_memory"
    });
    assert_eq!(upload(&renderer).unwrap().row[7], text("out_of_memory"));

    let mut bare = panic_json();
    bare.as_object_mut().unwrap().remove("location");
    assert_eq!(upload(&bare).unwrap().row[7], text("panic"));
}

#[test]
fn anything_the_crate_refuses_is_bad() {
    let mut extra = panic_json();
    extra["message"] = json!("called `Option::unwrap()` on a `None` value");
    let mut old = panic_json();
    old["week"] = json!("2026-W40");
    let mut install = panic_json();
    install["install"] = json!("8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40");
    for bad in [extra, old, install] {
        assert_eq!(upload(&bad), Err(Refusal::Bad), "{bad}");
    }
    assert_eq!(accept(b"not json", now()), Err(Refusal::Bad));
}

#[test]
fn the_hour_sorts_as_it_reads() {
    assert_eq!(hour(now()).unwrap(), "2026-11-11T14");
    let midnight = Day::from_ymd(2026, 11, 12).unwrap().start();
    assert_eq!(hour(midnight).unwrap(), "2026-11-12T00");
    assert!(hour(now()).unwrap() < hour(midnight).unwrap());
    assert_eq!(BUMP_CRASH_RATE.matches('?').count(), 2);
    assert!(INSERT_CRASH.starts_with("INSERT OR IGNORE"));
}

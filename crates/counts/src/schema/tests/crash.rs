//! The crash report's own rules: the strict read and its example per kind, the source file's
//! pattern at its edges, the kind's fields, the week's window, the size bound, and arbitrary
//! reports against the committed schema.

use std::collections::BTreeSet;

use proptest::prelude::*;
use serde_json::{Value, json};

use super::sample::{Sample, Tape};
use super::wire::{breaks_schema, crash_schema};
use crate::schema::{
    Crash, CrashArea, CrashKind, CrashProcess, CrashReport, Day, Invalid, MAX_CRASH_JSON,
    MAX_UNIX_SIGNAL, Platform, SOURCE_FILE_MAX, SOURCE_FILE_PATTERN, SignalCode, SourceFile, Week,
    crash_body, parse_crash,
};

fn this_week() -> Week {
    Day::from_ymd(2026, 11, 2).unwrap().week()
}

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

fn signal_json() -> Value {
    let mut report = panic_json();
    let object = report.as_object_mut().unwrap();
    object.remove("location");
    object.insert("kind".to_owned(), json!("signal"));
    object.insert("area".to_owned(), json!("other"));
    object.insert("code".to_owned(), json!(3_221_225_477_u32));
    report
}

fn renderer_json() -> Value {
    json!({
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
    })
}

fn parse(report: &Value) -> Result<CrashReport, Invalid> {
    parse_crash(&serde_json::to_vec(report).unwrap(), this_week())
}

fn with(mut report: Value, pointer: &str, value: Value) -> Value {
    *report.pointer_mut(pointer).unwrap() = value;
    report
}

#[test]
fn each_example_is_taken_and_written_back_the_same() {
    for example in [panic_json(), signal_json(), renderer_json()] {
        let report = parse(&example).unwrap();
        assert_eq!(serde_json::to_value(&report).unwrap(), example);
        let body = crash_body(&report, this_week()).unwrap();
        assert_eq!(body, serde_json::to_vec(&report).unwrap());
        assert_eq!(parse_crash(&body, this_week()), Ok(report));
    }
    let mut no_location = panic_json();
    no_location.as_object_mut().unwrap().remove("location");
    assert!(parse(&no_location).is_ok());
}

#[test]
fn an_unknown_field_anywhere_is_refused() {
    let planted = [
        ("install", json!("8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40")),
        ("tally", json!("1c0e7d52-9a43-4f1b-8e6d-3b2a9f7c4d18")),
        (
            "message",
            json!("called `Option::unwrap()` on a `None` value"),
        ),
        ("day", json!("2026-11-01")),
        ("thread", json!("host")),
    ];
    for (key, value) in planted {
        let mut report = panic_json();
        report[key] = value;
        assert_eq!(parse(&report), Err(Invalid::Schema), "{key}");
    }
    let mut column = panic_json();
    column["location"]["column"] = json!(9);
    assert_eq!(parse(&column), Err(Invalid::Schema));
}

#[test]
fn an_unknown_name_or_a_number_past_its_cap_is_refused() {
    let cases = [
        ("/area", json!("fake_agent")),
        ("/area", json!("Store")),
        ("/process", json!("gpu")),
        ("/kind", json!("oom")),
        ("/location/line", json!(0)),
        ("/location/line", json!(1_000_000)),
        ("/location/line", json!("212")),
        ("/report", json!("3d1f8a62-5c4b-1e7a-b9d0-1f2e3a4b5c6d")),
        ("/week", json!("2026-11-02")),
        ("/week", json!("2026-W45-1")),
        ("/schema", json!(2)),
        ("/after_launch", json!(1)),
    ];
    for (pointer, value) in cases {
        let report = with(panic_json(), pointer, value.clone());
        assert_eq!(parse(&report), Err(Invalid::Schema), "{pointer} = {value}");
    }
    assert_eq!(
        parse(&with(signal_json(), "/code", json!(0))),
        Err(Invalid::Schema)
    );
    assert_eq!(
        parse(&with(signal_json(), "/code", json!(4_294_967_296_u64))),
        Err(Invalid::Schema)
    );
    assert_eq!(
        parse(&with(signal_json(), "/code", json!(-11))),
        Err(Invalid::Schema)
    );
    assert_eq!(
        parse(&with(renderer_json(), "/reason", json!("killed"))),
        Err(Invalid::Schema)
    );
}

#[test]
fn a_source_file_is_taken_only_in_its_two_forms() {
    let longest_segment = "a".repeat(64);
    let deepest = format!("crates/host/src/{}x.rs", "a/".repeat(10));
    let taken = [
        "crates/host/src/log_file/panic.rs".to_owned(),
        "crates/codex_appserver/src/lib.rs".to_owned(),
        "crates/desktop/src/main.rs".to_owned(),
        "tokio-1.54.1/src/runtime/task/harness.rs".to_owned(),
        "windows-sys-0.59.0/src/Windows/Win32/Foundation/mod.rs".to_owned(),
        "h2-0.4.6/src/proto/streams/recv.rs".to_owned(),
        "serde_json-1.0.140/src/de.rs".to_owned(),
        "x-0.1.0-rc.1/src/lib.rs".to_owned(),
        "x-0.0.0-a.b.c.d/src/lib.rs".to_owned(),
        "a-123456789.0.0/src/src/lib.rs".to_owned(),
        format!("crates/store/src/{longest_segment}.rs"),
        deepest.clone(),
    ];
    for text in &taken {
        assert!(text.parse::<SourceFile>().is_ok(), "{text}");
    }
    let too_deep = deepest.replace("src/a/", "src/a/a/");
    let too_long = format!("crates/store/src/{}b.rs", ("a".repeat(60) + "/").repeat(3));
    assert!(too_long.len() > SOURCE_FILE_MAX);
    let refused = [
        String::new(),
        // unix-paths-ok: a string the pattern must refuse; it never reaches a path API.
        "/home/priya/clauseo/crates/host/src/lib.rs".to_owned(),
        "C:\\Users\\priya\\crates\\host\\src\\lib.rs".to_owned(),
        "crates\\host\\src\\lib.rs".to_owned(),
        "crates/host/src/../../../Mehra/brief.rs".to_owned(),
        "crates/host/src/./lib.rs".to_owned(),
        "crates/host/src//lib.rs".to_owned(),
        "crates/host/src/.rs".to_owned(),
        "crates/host/src/lib.txt".to_owned(),
        "crates/host/src/lib.rs ".to_owned(),
        "crates/host/src/my-file.rs".to_owned(),
        "crates/host/src/Mehra divorce.rs".to_owned(),
        "crates/host/tests/rig.rs".to_owned(),
        "crates/host/build.rs".to_owned(),
        "crates/fake_agent/src/main.rs".to_owned(),
        "crates/webview/src/lib.rs".to_owned(),
        "crates/Host/src/lib.rs".to_owned(),
        "library/std/src/thread/mod.rs".to_owned(),
        "tokio/src/lib.rs".to_owned(),
        "tokio-1.54/src/lib.rs".to_owned(),
        "tokio-01.54.1/src/lib.rs".to_owned(),
        "tokio-1.54.1+build.5/src/lib.rs".to_owned(),
        "tokio-1.54.1-/src/lib.rs".to_owned(),
        "tokio-1.54.1-a.b.c.d.e/src/lib.rs".to_owned(),
        "tokio-1.54.1-rc_1/src/lib.rs".to_owned(),
        "tokio-1234567890.0.0/src/lib.rs".to_owned(),
        "1tokio-1.54.1/src/lib.rs".to_owned(),
        "-1.54.1/src/lib.rs".to_owned(),
        format!("{}-1.0.0/src/lib.rs", "a".repeat(65)),
        format!("crates/store/src/{longest_segment}a.rs"),
        "index.crates.io-6f17d22bba15001f/tokio-1.54.1/src/lib.rs".to_owned(),
        "tokio-1.54.1/src/lib.rs\n".to_owned(),
        "tokio-1.54.1/src/λ.rs".to_owned(),
        too_deep,
        too_long,
    ];
    for text in &refused {
        assert!(text.parse::<SourceFile>().is_err(), "{text:?}");
        let report = with(panic_json(), "/location/file", json!(text));
        assert_eq!(parse(&report), Err(Invalid::Schema), "{text:?}");
    }
}

#[test]
fn a_source_file_names_its_crate() {
    let file: SourceFile = "crates/host/src/log_file/panic.rs".parse().unwrap();
    assert_eq!(file.area(), Some(CrashArea::Host));
    let dep: SourceFile = "tokio-1.54.1/src/lib.rs".parse().unwrap();
    assert_eq!(dep.area(), None);
    assert_eq!(CrashArea::from_crate_dir("webview"), None);
    assert_eq!(CrashArea::from_crate_dir("other"), None);
    assert_eq!(CrashArea::from_crate_dir("fake_agent"), None);
}

/// The pattern's alternation of crate names, as a set.
fn pattern_crates() -> BTreeSet<&'static str> {
    let start = SOURCE_FILE_PATTERN.find("crates/(").unwrap() + "crates/(".len();
    let end = start + SOURCE_FILE_PATTERN[start..].find(')').unwrap();
    SOURCE_FILE_PATTERN[start..end].split('|').collect()
}

#[test]
fn the_pattern_lists_exactly_the_areas_crates() {
    let areas: BTreeSet<&str> = CrashArea::ALL
        .iter()
        .filter_map(|area| CrashArea::from_crate_dir(area.as_str()).map(CrashArea::as_str))
        .collect();
    assert_eq!(pattern_crates(), areas);
    assert_eq!(areas.len(), CrashArea::ALL.len() - 2);
}

#[test]
fn each_kind_has_its_own_fields_and_no_other() {
    let mut no_code = signal_json();
    no_code.as_object_mut().unwrap().remove("code");
    let mut no_reason = renderer_json();
    no_reason.as_object_mut().unwrap().remove("reason");
    let mut panic_with_reason = panic_json();
    panic_with_reason["reason"] = json!("crashed");
    let mut signal_with_location = signal_json();
    signal_with_location["location"] = panic_json()["location"].clone();
    let mut panic_with_code = panic_json();
    panic_with_code["code"] = json!(6);
    let mut renderer_with_code = renderer_json();
    renderer_with_code["code"] = json!(11);
    let mut stall = renderer_json();
    stall.as_object_mut().unwrap().remove("reason");
    stall["kind"] = json!("stall");
    let refused = [
        with(panic_json(), "/process", json!("webview")),
        panic_with_code,
        panic_with_reason,
        no_code,
        signal_with_location,
        with(signal_json(), "/process", json!("webview")),
        no_reason,
        renderer_with_code,
        with(renderer_json(), "/process", json!("host")),
        with(renderer_json(), "/area", json!("host")),
        stall,
    ];
    for report in refused {
        assert_eq!(parse(&report), Err(Invalid::Detail), "{report}");
    }
}

#[test]
fn a_unix_signal_is_1_to_64_and_a_windows_code_any() {
    let mut mac = signal_json();
    mac["os"] = json!("macos");
    mac["os_major"] = json!(15);
    assert!(parse(&with(mac.clone(), "/code", json!(11))).is_ok());
    assert!(parse(&with(mac.clone(), "/code", json!(MAX_UNIX_SIGNAL))).is_ok());
    assert_eq!(parse(&with(mac, "/code", json!(65))), Err(Invalid::Detail));
    let mut linux = signal_json();
    linux["os"] = json!("linux");
    linux.as_object_mut().unwrap().remove("os_major");
    assert_eq!(parse(&linux), Err(Invalid::Detail));
    assert!(parse(&with(linux, "/code", json!(6))).is_ok());
    assert!(parse(&with(signal_json(), "/code", json!(u32::MAX))).is_ok());
}

#[test]
fn the_week_is_this_one_or_at_most_5_before() {
    let week = |age: i64| json!(this_week().weeks_before(age).unwrap().to_string());
    for age in 0..=5 {
        assert!(
            parse(&with(panic_json(), "/week", week(age))).is_ok(),
            "{age}"
        );
    }
    for age in [-1, 6, 52] {
        let report = with(panic_json(), "/week", week(age));
        assert_eq!(parse(&report), Err(Invalid::OutOfWindow), "{age}");
    }
    let mut linux = panic_json();
    linux["os"] = json!("linux");
    assert_eq!(parse(&linux), Err(Invalid::Platform));
    assert_eq!(
        parse(&with(panic_json(), "/os_major", json!(13))),
        Err(Invalid::Platform)
    );
}

/// The longest report the types allow: the longest source file and the widest numbers.
fn largest() -> Value {
    let mut report = panic_json();
    let file = format!(
        "crates/codex_appserver/src/{}",
        ("a".repeat(63) + "/").repeat(2)
    );
    let file = format!("{file}{}.rs", "b".repeat(SOURCE_FILE_MAX - file.len() - 3));
    assert_eq!(file.len(), SOURCE_FILE_MAX);
    report["version"] = json!("999.999.999");
    report["location"] = json!({ "file": file, "line": 999_999 });
    report
}

#[test]
fn a_report_is_far_under_its_cap_and_the_cap_far_under_128_kb() {
    const { assert!(MAX_CRASH_JSON * 32 < 128 * 1024) };
    let largest = parse(&largest()).unwrap();
    let body = crash_body(&largest, this_week()).unwrap();
    assert!(body.len() < 1_000, "{}", body.len());
    let mut code = signal_json();
    code["code"] = json!(u32::MAX);
    assert!(serde_json::to_vec(&code).unwrap().len() < 1_000);
    assert_eq!(
        parse_crash(&vec![b' '; MAX_CRASH_JSON + 1], this_week()),
        Err(Invalid::TooLarge)
    );
}

#[test]
fn crash_body_refuses_what_parse_crash_refuses() {
    let mut report = parse(&panic_json()).unwrap();
    report.kind = CrashKind::RendererGone;
    assert_eq!(crash_body(&report, this_week()), Err(Invalid::Detail));
    let old = parse(&panic_json()).unwrap();
    let later = this_week().weeks_before(-6).unwrap();
    assert_eq!(crash_body(&old, later), Err(Invalid::OutOfWindow));
}

#[test]
fn the_tally_event_is_the_reports_own_three_fields() {
    let report = parse(&renderer_json()).unwrap();
    let event = Crash {
        process: CrashProcess::Webview,
        kind: CrashKind::RendererGone,
        after_launch: false,
    };
    assert_eq!(report.tally_event(), event);
}

#[test]
fn the_schema_has_no_id_text_or_time_field() {
    let schema = crash_schema();
    let fields: BTreeSet<&str> = schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for field in [
        "install", "tally", "message", "day", "time", "path", "thread", "language",
    ] {
        assert!(!fields.contains(field), "the crash report has {field}");
    }
    assert_eq!(fields.len(), 15);
}

/// An arbitrary report that keeps every rule: the kind's own field, a unix code in range.
fn arbitrary_report(tape: &mut Tape) -> CrashReport {
    let mut report = CrashReport::sample(tape);
    let platform = Platform::sample(tape);
    report.os = platform.os();
    report.os_major = platform.os_major();
    report.week = this_week()
        .weeks_before(i64::try_from(tape.below(6)).unwrap())
        .unwrap();
    report.kind = tape.pick(&[CrashKind::Panic, CrashKind::Signal, CrashKind::RendererGone]);
    match report.kind {
        CrashKind::Panic | CrashKind::Signal => report.process = CrashProcess::Host,
        _ => {
            report.process = CrashProcess::Webview;
            report.area = CrashArea::Webview;
        }
    }
    if report.kind != CrashKind::Panic {
        report.location = None;
    }
    report.code = (report.kind == CrashKind::Signal).then(|| {
        let max = if report.os == crate::schema::Os::Windows {
            u64::from(u32::MAX)
        } else {
            u64::from(MAX_UNIX_SIGNAL)
        };
        SignalCode::new(u32::try_from(1 + tape.below(max)).unwrap()).unwrap()
    });
    report.reason = (report.kind == CrashKind::RendererGone).then(|| Sample::sample(tape));
    report
}

proptest! {
    #[test]
    fn arbitrary_reports_fit_their_schema_field_by_field(
        values in proptest::collection::vec(any::<u64>(), 0..60),
    ) {
        let mut tape = Tape::new(values);
        let report = arbitrary_report(&mut tape);
        let body = crash_body(&report, this_week()).unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        prop_assert_eq!(breaks_schema(&crash_schema(), &json), Vec::<String>::new());
        prop_assert_eq!(parse_crash(&body, this_week()), Ok(report));
        prop_assert!(body.len() < 1_000);
    }
}

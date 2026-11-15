//! Crash reports (decision record 0060): recording, the native record, the move to the queue and
//! the tally, and what off and policy delete. The sending is in `crash_send.rs`.

use std::io::Write as _;

use clauseo_counts::schema::{
    CrashArea, CrashKind, CrashProcess, CrashReport, MAX_CRASH_JSON, RendererGoneReason,
    SourceFile, Time, parse_crash,
};
use clauseo_counts::{
    Counts, Crashes, MAX_QUEUED, NATIVE_LINE_MAX, Switch, area_of_thread, native_line, source_file,
};
use serde_json::Value;

use crate::support::{HOUR, SEED, Scratch, about, at, counts_dir, files, plant, saving};

pub(crate) const ON: Switch = Switch {
    counts: true,
    locked_off: false,
};
pub(crate) const LOCKED: Switch = Switch {
    counts: true,
    locked_off: true,
};
pub(crate) const OFF: Switch = Switch {
    counts: false,
    locked_off: false,
};

/// Counts saving and crashes recording, as desktop has them after its first tick.
pub(crate) fn recording(scratch: &Scratch) -> (Counts, Crashes) {
    let counts = saving(scratch);
    let crashes = Crashes::open(scratch.path(), about(), SEED);
    crashes.ingest(&counts, ON, at(2, 0)).unwrap();
    assert!(crashes.recording());
    (counts, crashes)
}

pub(crate) fn crash_files(scratch: &Scratch) -> Vec<String> {
    files(&scratch.path().join("crash"))
}

/// A renderer gone at `now`, recorded by another run of the app (one per run) into the same
/// data folder, for tests that need several reports.
pub(crate) fn renderer_gone_in_a_new_run(scratch: &Scratch, now: Time) {
    let run = Crashes::open(scratch.path(), about(), SEED);
    run.set_recording(true);
    run.record_renderer_gone(RendererGoneReason::Crashed, now)
        .unwrap();
}

/// A panic in the store at `now`, written.
pub(crate) fn panic_at(crashes: &Crashes, now: Time) {
    crashes
        .prepare_panic(Some("crates/store/src/writer.rs"), 212, Some("main"), now)
        .unwrap()
        .write()
        .unwrap();
}

/// The report in the queue file `name`.
pub(crate) fn queued_report(scratch: &Scratch, name: &str) -> CrashReport {
    let bytes = std::fs::read(scratch.path().join("crash/queue").join(name)).unwrap();
    parse_crash(&bytes, at(2, 0).day().unwrap().week()).unwrap()
}

/// The `crash` cells of the week's tally file, once saved.
fn tally_crash_cells(scratch: &Scratch, counts: &Counts, now: Time) -> Value {
    fn find(value: &Value) -> Option<Value> {
        match value {
            Value::Object(object) => object
                .get("crash")
                .cloned()
                .or_else(|| object.values().find_map(find)),
            Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    counts.tick(now).unwrap();
    let path = counts_dir(scratch).join("tally/2026-W45.json");
    let json: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    find(&json).unwrap_or(Value::Null)
}

#[test]
fn a_panic_path_is_made_relative_or_left_out() {
    let cases = [
        ("crates/host/src/actor.rs", Some("crates/host/src/actor.rs")),
        (
            "crates\\host\\src\\actor\\turn.rs",
            Some("crates/host/src/actor/turn.rs"),
        ),
        (
            // unix-paths-ok: a panic location string; source_file only edits strings.
            "/home/runner/work/clauseo/crates/store/src/lib.rs",
            Some("crates/store/src/lib.rs"),
        ),
        (
            // unix-paths-ok: a panic location string; source_file only edits strings.
            "/home/u/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.54.1/src/lib.rs",
            Some("tokio-1.54.1/src/lib.rs"),
        ),
        (
            "C:\\Users\\u\\.cargo\\registry\\src\\index.crates.io-1949cf8c6b5b557f\\h2-0.4.6\\src\\a.rs",
            Some("h2-0.4.6/src/a.rs"),
        ),
        (
            "tokio-1.54.1/src/runtime/task.rs",
            Some("tokio-1.54.1/src/runtime/task.rs"),
        ),
        ("/rustc/abc123/library/core/src/option.rs", None),
        (
            // unix-paths-ok: a panic location string; source_file only edits strings.
            "/home/u/.cargo/git/checkouts/thing-1a2b/3c4d/src/lib.rs",
            None,
        ),
        // unix-paths-ok: a panic location string; source_file only edits strings.
        ("/home/u/Mehra/crates/fake_agent/src/main.rs", None),
        ("", None),
    ];
    for (raw, want) in cases {
        let got = source_file(raw);
        assert_eq!(got.as_ref().map(SourceFile::as_str), want, "{raw}");
    }
}

#[test]
fn a_thread_names_its_area_by_the_fixed_list() {
    assert_eq!(area_of_thread(Some("main")), CrashArea::Desktop);
    assert_eq!(area_of_thread(Some("clauseo-host-start")), CrashArea::Host);
    assert_eq!(
        area_of_thread(Some("clauseo-store-writer")),
        CrashArea::Store
    );
    assert_eq!(
        area_of_thread(Some("clauseo-relay-call")),
        CrashArea::Connectors
    );
    assert_eq!(
        area_of_thread(Some("tokio-runtime-worker")),
        CrashArea::Other
    );
    assert_eq!(area_of_thread(None), CrashArea::Other);
}

#[test]
fn a_panic_is_recorded_once_then_queued_and_tallied() {
    let scratch = Scratch::new("crash-panic");
    let (counts, crashes) = recording(&scratch);
    crashes.set_launched();
    panic_at(&crashes, at(2, HOUR));
    // One report per run: a second panic (or the abort after it) makes none.
    assert!(crashes.crashed());
    assert!(crashes.prepare_panic(None, 1, None, at(2, HOUR)).is_none());
    assert!(!crashes.begin_crash());
    let new = crash_files(&scratch);
    assert_eq!(new.len(), 1);
    assert!(new[0].starts_with("new/"), "{new:?}");

    crashes.ingest(&counts, ON, at(2, HOUR + 60)).unwrap();
    let queued = crashes.queued();
    assert_eq!(queued.len(), 1);
    let report = queued_report(&scratch, &queued[0]);
    assert_eq!(report.kind, CrashKind::Panic);
    assert_eq!(report.process, CrashProcess::Host);
    assert_eq!(report.area, CrashArea::Store);
    assert!(report.after_launch);
    let location = report.location.unwrap();
    assert_eq!(location.file.as_str(), "crates/store/src/writer.rs");
    assert_eq!(location.line.get(), 212);
    let cells = tally_crash_cells(&scratch, &counts, at(2, HOUR + 120));
    assert_eq!(
        cells,
        serde_json::json!([{ "process": "host", "kind": "panic", "after_launch": true, "n": 1 }])
    );
    // A later tick counts it no more.
    crashes.ingest(&counts, ON, at(2, HOUR + 180)).unwrap();
    let again = tally_crash_cells(&scratch, &counts, at(2, HOUR + 240));
    assert_eq!(again[0]["n"], 1);
}

#[test]
fn a_dependency_panic_takes_its_area_from_the_thread() {
    let scratch = Scratch::new("crash-dep");
    let (counts, crashes) = recording(&scratch);
    // unix-paths-ok: a panic location string; source_file only edits strings.
    let raw = "/home/u/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/zip-9.2.2/src/read.rs";
    let pending = crashes.prepare_panic(Some(raw), 77, Some("clauseo-host-start"), at(2, 60));
    pending.unwrap().write().unwrap();
    crashes.ingest(&counts, ON, at(2, 120)).unwrap();
    let report = queued_report(&scratch, &crashes.queued()[0]);
    assert_eq!(report.area, CrashArea::Host);
    assert!(!report.after_launch);
    assert_eq!(
        report.location.unwrap().file.as_str(),
        "zip-9.2.2/src/read.rs"
    );
}

#[test]
fn a_renderer_gone_is_recorded_once_per_run() {
    let scratch = Scratch::new("crash-renderer");
    let (counts, crashes) = recording(&scratch);
    for reason in [RendererGoneReason::Crashed, RendererGoneReason::OutOfMemory] {
        crashes.record_renderer_gone(reason, at(2, 60)).unwrap();
    }
    // A renderer gone is not the host's crash: a panic after it still makes its report.
    assert!(!crashes.crashed());
    crashes.ingest(&counts, ON, at(2, 120)).unwrap();
    let queued = crashes.queued();
    assert_eq!(
        queued.len(),
        1,
        "a renderer that keeps failing makes one report"
    );
    let report = queued_report(&scratch, &queued[0]);
    assert_eq!(report.process, CrashProcess::Webview);
    assert_eq!(report.area, CrashArea::Webview);
    assert_eq!(report.reason, Some(RendererGoneReason::Crashed));
    let cells = tally_crash_cells(&scratch, &counts, at(2, 180));
    assert_eq!(cells[0]["n"], 1);
    panic_at(&crashes, at(2, 240));
    // The next run records one again.
    renderer_gone_in_a_new_run(&scratch, at(2, 300));
    crashes.ingest(&counts, ON, at(2, 360)).unwrap();
    assert_eq!(crashes.queued().len(), 3);
}

#[test]
fn the_native_line_has_one_fixed_form() {
    let mut buf = [0_u8; NATIVE_LINE_MAX];
    assert_eq!(native_line(11, false, &mut buf), b"signal 11 0\n");
    assert_eq!(native_line(0, true, &mut buf), b"signal 0 1\n");
    assert_eq!(
        native_line(u32::MAX, true, &mut buf),
        b"signal 4294967295 1\n"
    );
}

#[test]
fn a_native_crash_becomes_a_report_at_the_next_start_with_its_own_version() {
    let scratch = Scratch::new("crash-native");
    let (counts, crashes) = recording(&scratch);
    let mut record = crashes.open_native(at(2, 0)).unwrap().unwrap();
    // The handler's write.
    let mut buf = [0_u8; NATIVE_LINE_MAX];
    record
        .write_all(native_line(3_221_225_477, true, &mut buf))
        .unwrap();
    drop(record);

    // The next start, after an update.
    let mut updated = about();
    updated.platform = clauseo_counts::schema::Platform::windows(11).unwrap();
    updated.version = "3.0.5".parse().unwrap();
    let next = Crashes::open(scratch.path(), updated, SEED);
    next.ingest(&counts, ON, at(2, HOUR)).unwrap();
    let _record = next.open_native(at(2, HOUR)).unwrap().unwrap();
    next.ingest(&counts, ON, at(2, HOUR + 60)).unwrap();
    let report = queued_report(&scratch, &next.queued()[0]);
    assert_eq!(report.kind, CrashKind::Signal);
    assert_eq!(report.code.unwrap().get(), 3_221_225_477);
    assert_eq!(report.version.to_string(), "3.0.4");
    assert!(report.after_launch);
    assert_eq!(report.area, CrashArea::Other);
}

#[test]
fn a_native_record_with_no_line_or_another_form_makes_no_report() {
    let scratch = Scratch::new("crash-native-clean");
    let (counts, crashes) = recording(&scratch);
    // A clean exit, a kill or a power loss leaves the header alone.
    drop(crashes.open_native(at(2, 0)).unwrap().unwrap());
    let native = scratch.path().join("crash/native");
    for planted in [
        None,
        Some(&b"clauseo-native 1 3.0.4\nsignal x 1\n"[..]),
        Some(&b"clauseo-native 1 3.0.4\nsignal 11 2\n"[..]),
        Some(&b"clauseo-native 1 3.0.4\nsignal 0 1\n"[..]),
        Some(&b"clauseo-native 2 3.0.4\nsignal 11 1\n"[..]),
        Some(&b"\xff\xfe"[..]),
    ] {
        if let Some(bytes) = planted {
            plant(&native, bytes);
        }
        let next = Crashes::open(scratch.path(), about(), SEED);
        next.ingest(&counts, ON, at(2, HOUR)).unwrap();
        drop(next.open_native(at(2, HOUR)).unwrap().unwrap());
        next.ingest(&counts, ON, at(2, HOUR + 60)).unwrap();
        assert_eq!(next.queued(), Vec::<String>::new());
    }
}

#[test]
fn nothing_is_recorded_before_the_first_act_or_with_counts_off() {
    let scratch = Scratch::new("crash-not-recording");
    let memory = Counts::open(scratch.path(), about(), true, false, SEED);
    let crashes = Crashes::open(scratch.path(), about(), SEED);
    assert!(!crashes.recording());
    crashes.ingest(&memory, ON, at(2, 0)).unwrap();
    assert!(!crashes.recording());
    assert!(crashes.prepare_panic(None, 1, None, at(2, 60)).is_none());
    assert!(!crashes.crashed());
    crashes
        .record_renderer_gone(RendererGoneReason::Crashed, at(2, 60))
        .unwrap();
    assert!(crashes.open_native(at(2, 60)).unwrap().is_none());
    assert_eq!(crash_files(&scratch), Vec::<String>::new());

    let counts = saving(&scratch);
    crashes.ingest(&counts, OFF, at(2, 0)).unwrap();
    assert!(!crashes.recording());
}

#[test]
fn counts_off_deletes_every_report_and_stops_recording() {
    let scratch = Scratch::new("crash-off");
    let (counts, crashes) = recording(&scratch);
    crashes
        .record_renderer_gone(RendererGoneReason::Crashed, at(2, 60))
        .unwrap();
    crashes.ingest(&counts, ON, at(2, 120)).unwrap();
    panic_at(&crashes, at(2, 180));
    drop(crashes.open_native(at(2, 180)).unwrap());
    assert_ne!(crash_files(&scratch), Vec::<String>::new());
    crashes.ingest(&counts, OFF, at(2, 240)).unwrap();
    assert!(!scratch.path().join("crash").exists());
    assert!(!crashes.recording());
}

#[test]
fn a_policy_lock_deletes_the_queue_and_still_tallies() {
    let scratch = Scratch::new("crash-locked");
    let (counts, crashes) = recording(&scratch);
    crashes
        .record_renderer_gone(RendererGoneReason::Crashed, at(2, 60))
        .unwrap();
    crashes.ingest(&counts, ON, at(2, 120)).unwrap();
    assert_eq!(crashes.queued().len(), 1);
    renderer_gone_in_a_new_run(&scratch, at(2, 180));
    crashes.ingest(&counts, LOCKED, at(2, 240)).unwrap();
    assert_eq!(crashes.queued(), Vec::<String>::new());
    assert_eq!(crash_files(&scratch), Vec::<String>::new());
    // Still recording, so the tally counts what the lock keeps from being sent.
    assert!(crashes.recording());
    let cells = tally_crash_cells(&scratch, &counts, at(2, 300));
    assert_eq!(cells[0]["n"], 2);
}

#[test]
fn at_most_5_wait_and_none_past_30_days() {
    let scratch = Scratch::new("crash-cap");
    let (counts, crashes) = recording(&scratch);
    for n in 0..7 {
        renderer_gone_in_a_new_run(&scratch, at(2, 60 + n));
    }
    // A temp file a crash cut short goes too.
    plant(&scratch.path().join("crash/new/.x.json.1-0.tmp"), b"{");
    crashes.ingest(&counts, ON, at(2, 120)).unwrap();
    assert_eq!(crashes.queued().len(), MAX_QUEUED);
    assert_eq!(
        files(&scratch.path().join("crash/new")),
        Vec::<String>::new()
    );
    let cells = tally_crash_cells(&scratch, &counts, at(2, 180));
    assert_eq!(cells[0]["n"], 7);
    crashes.ingest(&counts, ON, at(2 + 30, 59)).unwrap();
    assert_eq!(crashes.queued().len(), MAX_QUEUED);
    crashes.ingest(&counts, ON, at(2 + 30, 120)).unwrap();
    assert_eq!(crashes.queued(), Vec::<String>::new());
}

#[test]
fn a_report_that_does_not_read_is_deleted_untallied() {
    let scratch = Scratch::new("crash-bad");
    let (counts, crashes) = recording(&scratch);
    let name = "7200-3d1f8a62-5c4b-4e7a-b9d0-1f2e3a4b5c6d.json";
    plant(
        &scratch.path().join("crash/new").join(name),
        br#"{"schema":1,"message":"Mehra"}"#,
    );
    crashes.ingest(&counts, ON, at(2, 60)).unwrap();
    assert_eq!(crash_files(&scratch), Vec::<String>::new());
    let cells = tally_crash_cells(&scratch, &counts, at(2, 120));
    assert!(cells.as_array().is_none_or(Vec::is_empty), "{cells}");
}

/// The native record, a link to a file outside `crash/` (a matter's paper): the next start
/// deletes the link and makes the record anew, so the paper is never truncated or written.
#[cfg(unix)]
#[test]
fn a_link_in_place_of_the_native_record_is_never_followed() {
    let scratch = Scratch::new("crash-native-link");
    let (_counts, crashes) = recording(&scratch);
    let paper = scratch.path().join("matter/contract.docx");
    plant(&paper, b"the lawyer's paper");
    let native = scratch.path().join("crash/native");
    crate::write::link_file(&native, &paper);

    let mut record = crashes.open_native(at(2, 0)).unwrap().unwrap();
    let mut buf = [0_u8; NATIVE_LINE_MAX];
    record.write_all(native_line(11, true, &mut buf)).unwrap();
    drop(record);

    assert_eq!(std::fs::read(&paper).unwrap(), b"the lawyer's paper");
    let kind = std::fs::symlink_metadata(&native).unwrap().file_type();
    assert!(kind.is_file() && !kind.is_symlink(), "{kind:?}");
    assert!(std::fs::read(&native).unwrap().ends_with(b"signal 11 1\n"));
}

/// Files past `MAX_CRASH_JSON` bytes are never read whole: a new report, a queued one and a
/// native record that begins well and runs on are each deleted, with no report and no POST.
#[test]
fn an_oversized_file_is_deleted_unread() {
    let scratch = Scratch::new("crash-oversized");
    let (counts, _crashes) = recording(&scratch);
    let big = |head: &[u8]| {
        let mut bytes = head.to_vec();
        bytes.resize(MAX_CRASH_JSON * 64, b' ');
        bytes
    };
    let name = format!(
        "{}-3d1f8a62-5c4b-4e7a-b9d0-1f2e3a4b5c6d.json",
        at(2, 0).unix_secs()
    );
    plant(&scratch.path().join("crash/new").join(&name), &big(b"{"));
    plant(&scratch.path().join("crash/queue").join(&name), &big(b"{"));
    let native = scratch.path().join("crash/native");
    plant(&native, &big(b"clauseo-native 1 3.0.4\nsignal 11 1\n"));

    let next = Crashes::open(scratch.path(), about(), SEED);
    next.ingest(&counts, ON, at(2, 60)).unwrap();
    // The queued file is only read when it is due.
    assert_eq!(next.queued(), vec![name]);
    drop(next.open_native(at(2, 60)).unwrap().unwrap());
    assert!(std::fs::metadata(&native).unwrap().len() < 100);
    next.ingest(&counts, ON, at(2, 120)).unwrap();
    // The first try falls within the hour after the first look; every look finds nothing.
    let mut now = at(3, 60);
    while now <= at(3, 2 * HOUR) {
        assert_eq!(next.due(&counts, now), None);
        now = now.plus(60);
    }
    assert_eq!(next.queued(), Vec::<String>::new());
    assert_eq!(crash_files(&scratch), vec!["native".to_owned()]);
    let cells = tally_crash_cells(&scratch, &counts, at(3, 60));
    assert!(cells.as_array().is_none_or(Vec::is_empty), "{cells}");
}

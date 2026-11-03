//! The opt-in phase timings (clauseo-ai/clauseo-acp-private#291): one more stderr line after
//! the guard's own, naming each phase that ran and what the walk read; nothing when off.

use std::ffi::{OsStr, OsString};

use serde_json::{Value, json};

use super::super::timings::{PREFIX, wanted};
use super::{DENY, Fixture, deny, run_timed};

/// The exit code and the stderr lines of one guard call on `hook` with timings `on`.
fn timed(hook: &Value, on: bool) -> (i32, Vec<String>) {
    let args: [OsString; 0] = [];
    let mut errors = Vec::new();
    let code = run_timed(&args, hook.to_string().as_bytes(), &mut errors, on);
    let errors = String::from_utf8(errors).unwrap();
    (code, errors.lines().map(str::to_owned).collect())
}

/// The phase names of a timings line, in order, each with a number.
fn phases(line: &str) -> Vec<String> {
    let (phases, _counts) = line
        .strip_prefix(PREFIX)
        .unwrap()
        .split_once(" | ")
        .unwrap();
    phases
        .split_whitespace()
        .map(|phase| {
            let (name, micros) = phase.split_once('=').unwrap();
            micros.parse::<u128>().unwrap();
            name.to_owned()
        })
        .collect()
}

#[test]
fn a_refused_write_has_its_reason_then_the_timings() {
    let fx = Fixture::new("timings-write");
    let hook = fx.hook(
        "Write",
        &json!({"file_path": "Contract.docx", "content": "new"}),
    );
    let (code, lines) = timed(&hook, true);
    assert_eq!(code, DENY);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(format!("{}\n", lines[0]), deny("Contract.docx"));
    assert_eq!(
        phases(&lines[1]),
        ["read", "parse", "targets", "resolve", "decide", "total"]
    );
    // A file tool lists nothing.
    assert!(
        lines[1].ends_with(" | entries=0 folders=0 owner_folders=0 checked=0"),
        "{}",
        lines[1]
    );
}

/// The fixture's matter holds `Contract.docx`, its owner file, `Free.docx` and `sub/Note.txt`:
/// five entries in two folders, one with an owner file and two files looked up beside it.
#[test]
fn a_shell_command_times_the_walk_and_counts_what_it_read() {
    let fx = Fixture::new("timings-shell");
    let hook = fx.hook("Bash", &json!({"command": "echo hi > new.txt"}));
    let (code, lines) = timed(&hook, true);
    assert_eq!(code, 0);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(
        phases(&lines[0]),
        [
            "read", "parse", "targets", "resolve", "walk", "decide", "total"
        ]
    );
    assert!(
        lines[0].ends_with(" | entries=5 folders=2 owner_folders=1 checked=2"),
        "{}",
        lines[0]
    );
}

#[test]
fn off_the_guard_writes_only_its_own_lines() {
    let fx = Fixture::new("timings-off");
    let held = fx.hook(
        "Write",
        &json!({"file_path": "Contract.docx", "content": "new"}),
    );
    let (code, lines) = timed(&held, false);
    assert_eq!((code, lines.len()), (DENY, 1), "{lines:?}");
    let free = fx.hook("Bash", &json!({"command": "echo hi > new.txt"}));
    assert_eq!(timed(&free, false), (0, Vec::new()));
}

/// Input it ignores still gets the timings, with the phases that ran.
#[test]
fn ignored_input_is_timed_too() {
    let mut errors = Vec::new();
    let args: [OsString; 0] = [];
    assert_eq!(run_timed(&args, &b"not json"[..], &mut errors, true), 0);
    let errors = String::from_utf8(errors).unwrap();
    let lines: Vec<&str> = errors.lines().collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(phases(lines[1]), ["read", "decide", "total"]);
}

#[test]
fn only_a_value_other_than_empty_or_zero_turns_them_on() {
    assert!(!wanted(None));
    assert!(!wanted(Some(OsStr::new(""))));
    assert!(!wanted(Some(OsStr::new("0"))));
    assert!(wanted(Some(OsStr::new("1"))));
    assert!(wanted(Some(OsStr::new("yes"))));
}

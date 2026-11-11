//! The transcript matcher and reader: a steer is found by an exact trimmed match in the words of
//! a line of a kind that means taken, stamped no earlier than its send less the skew; other
//! kinds, a tool's output, earlier lines and files that cannot be read show nothing. Each read
//! goes on from the mark, at most its cap, whole lines only.

use std::path::PathBuf;

use clauseo_protocol::{AgentSessionId, Timestamp};
use serde_json::{Value, json};

use super::{Place, Read, Resume, Session, Wanted, mark, read_on, shows, taken};
use crate::write;

/// 2026-11-11T10:00:00Z in milliseconds.
const SENT: i64 = 1_794_391_200_000;
const STEER: &str = "Focus on clause 4";

fn wanted() -> Wanted {
    Wanted::new(
        ["  Focus on clause 4\n"].into_iter(),
        Timestamp::new(SENT).unwrap(),
    )
}

fn user(text: &str, at: &str) -> Value {
    json!({
        "type": "user",
        "timestamp": at,
        "message": {"role": "user", "content": [{"type": "text", "text": text}]},
    })
}

#[test]
fn a_user_line_with_the_exact_text_shows_it() {
    assert!(shows(&user(STEER, "2026-11-11T10:00:01.000Z"), &wanted()));
    // Trimmed on both sides; a string content counts as much as a block.
    let plain = json!({"type": "user", "timestamp": "2026-11-11T10:00:01Z",
        "message": {"content": format!(" {STEER} ")}});
    assert!(shows(&plain, &wanted()));
}

#[test]
fn only_an_exact_match_counts() {
    let at = "2026-11-11T10:00:01.000Z";
    assert!(!shows(&user("Focus on clause 4.", at), &wanted()));
    assert!(!shows(&user("Please: Focus on clause 4", at), &wanted()));
    assert!(!shows(&user("focus on clause 4", at), &wanted()));
}

#[test]
fn a_line_stamped_before_the_send_less_the_skew_does_not_count() {
    // Two seconds of skew: 09:59:58 counts, 09:59:57.999 does not.
    assert!(shows(&user(STEER, "2026-11-11T09:59:58.000Z"), &wanted()));
    assert!(!shows(&user(STEER, "2026-11-11T09:59:57.999Z"), &wanted()));
    // No stamp, or one that is not a time, does not count either.
    let bare = json!({"type": "user", "message": {"content": STEER}});
    assert!(!shows(&bare, &wanted()));
    assert!(!shows(&user(STEER, "yesterday"), &wanted()));
}

#[test]
fn the_kinds_that_mean_taken_count_and_no_other() {
    let at = "2026-11-11T10:00:01.000Z";
    let attachment = json!({"type": "attachment", "timestamp": at,
        "attachment": {"type": "queued_command", "prompt": STEER}});
    let absorbed = json!({"type": "queue-operation", "timestamp": at, "operation": "remove",
        "reason": "absorbed_mid_turn", "content": STEER});
    assert!(shows(&attachment, &wanted()));
    assert!(shows(&absorbed, &wanted()));
    let others = [
        json!({"type": "queue-operation", "timestamp": at, "operation": "enqueue",
            "content": STEER}),
        json!({"type": "queue-operation", "timestamp": at, "operation": "dequeue",
            "content": STEER}),
        json!({"type": "queue-operation", "timestamp": at, "operation": "remove",
            "content": STEER}),
        json!({"type": "attachment", "timestamp": at,
            "attachment": {"type": "edited_text_file", "snippet": STEER}}),
        json!({"type": "assistant", "timestamp": at,
            "message": {"content": [{"type": "text", "text": STEER}]}}),
        json!({"type": "summary", "timestamp": at, "summary": STEER}),
        json!({"type": "last-prompt", "timestamp": at, "lastPrompt": STEER}),
        // The words are only where the kind keeps them.
        json!({"type": "attachment", "timestamp": at,
            "attachment": {"type": "queued_command", "note": STEER}}),
        json!({"type": "user", "timestamp": at, "note": STEER, "message": {"content": "Hi"}}),
    ];
    for line in &others {
        assert!(!shows(line, &wanted()), "{line}");
    }
}

#[test]
fn a_steer_with_no_text_is_never_found() {
    let blank = Wanted::new(["  "].into_iter(), Timestamp::new(SENT).unwrap());
    assert!(!shows(&user("  ", "2026-11-11T10:00:01Z"), &blank));
    assert!(!shows(&user("", "2026-11-11T10:00:01Z"), &blank));
}

#[test]
fn a_tool_result_holding_the_steer_text_does_not_show_it() {
    let at = "2026-11-11T10:00:01.000Z";
    // Claude Code logs a tool's output as a `user` line: its `tool_result` block, and the
    // result again beside the message.
    let results = [
        json!({"type": "user", "timestamp": at, "toolUseResult": {"stdout": STEER},
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": STEER}]}}),
        json!({"type": "user", "timestamp": at, "toolUseResult": STEER,
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1",
                    "content": [{"type": "text", "text": STEER}]}]}}),
    ];
    for line in &results {
        assert!(!shows(line, &wanted()), "{line}");
    }
    // A text block beside a tool result is the lawyer's own.
    let beside = json!({"type": "user", "timestamp": at, "message": {"content": [
        {"type": "tool_result", "tool_use_id": "t1", "content": "ok"},
        {"type": "text", "text": STEER}]}});
    assert!(shows(&beside, &wanted()));
    // A queued command's prompt may be blocks too.
    let blocks = json!({"type": "attachment", "timestamp": at, "attachment": {
        "type": "queued_command", "prompt": [{"type": "text", "text": STEER}]}});
    assert!(shows(&blocks, &wanted()));
}

/// A fresh folder under the temp folder for `test`.
fn scratch(test: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("clauseo-transcript-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

/// A session `sess-1` with its home and matter folder under `root`, and its transcript's path.
fn session(root: &std::path::Path) -> (Session, PathBuf) {
    let folder = root.join("matter");
    write::create_dir_all(&folder).unwrap();
    let session = Session {
        home: root.join("home"),
        folder,
        id: AgentSessionId::new("sess-1").unwrap(),
    };
    let path = session.transcripts().remove(0);
    write::create_dir_all(path.parent().unwrap()).unwrap();
    (session, path)
}

/// `lines`, one per line.
fn jsonl(lines: &[&str]) -> String {
    lines.iter().flat_map(|line| [line, "\n"]).collect()
}

/// Appends `text` to `path`.
fn append(path: &std::path::Path, text: &str) {
    let mut all = std::fs::read(path).unwrap_or_default();
    all.extend_from_slice(text.as_bytes());
    write::write_file(path, &all).unwrap();
}

#[test]
fn a_transcript_is_read_from_the_session_file_and_unreadable_ones_show_nothing() {
    let root = scratch("session");
    let (session, path) = session(&root);
    // No file yet: marked at its start.
    let places = mark(&session);
    assert!(places.iter().all(|place| place.resume == Resume::default()));
    assert!(!taken(vec![(places.clone(), wanted())])[0].found);
    // Not JSON lines, then a line of another kind: nothing.
    write::write_file(&path, b"not json\n{\"type\":\"assistant\"}\n").unwrap();
    assert!(!taken(vec![(places.clone(), wanted())])[0].found);
    let line = user(STEER, "2026-11-11T10:00:03Z").to_string();
    write::write_file(&path, jsonl(&["not json", &line]).as_bytes()).unwrap();
    assert!(taken(vec![(places, wanted())])[0].found);
    // A session id that is not a plain file name has no transcript to mark.
    let odd = Session {
        id: AgentSessionId::new("../sess-1").unwrap(),
        ..session
    };
    assert_eq!(mark(&odd), Vec::<Place>::new());
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn reading_goes_on_from_the_mark_past_a_large_tool_result() {
    let root = scratch("mark");
    let (session, path) = session(&root);
    // A line from before the steer went out is never read, whatever it holds.
    let before = user(STEER, "2026-11-11T10:00:03Z").to_string();
    write::write_file(&path, jsonl(&[&before]).as_bytes()).unwrap();
    let places = mark(&session);
    assert!(!taken(vec![(places.clone(), wanted())])[0].found);
    // Claude reads a large file while the steer waits: its result, well over 256 KiB, lands
    // before the steer's line, and another after it.
    let result = |id: &str| {
        json!({"type": "user", "timestamp": "2026-11-11T10:00:02Z", "message": {"content": [
            {"type": "tool_result", "tool_use_id": id, "content": "x".repeat(400 * 1024)}]}})
        .to_string()
    };
    let steer = user(STEER, "2026-11-11T10:00:04Z").to_string();
    append(&path, &jsonl(&[&result("t1"), &steer, &result("t2")]));
    let seen = taken(vec![(places, wanted())]).remove(0);
    assert!(seen.found);
    assert!(!seen.behind);
    let size = std::fs::metadata(&path).unwrap().len();
    assert_eq!(
        seen.places[0].resume,
        Resume {
            at: size,
            inside: false
        }
    );
    write::remove_dir_all(&root).unwrap();
}

/// What a read of `path` from `resume` with a cap of `cap` gives: its lines' `n`, and the next
/// resume.
fn read_ns(path: &std::path::Path, resume: Resume, cap: u64) -> (Vec<i64>, Resume, bool) {
    let Read {
        lines,
        next,
        behind,
    } = read_on(path, resume, cap).unwrap();
    let ns = lines.iter().filter_map(|line| line["n"].as_i64()).collect();
    (ns, next, behind)
}

/// A read that starts at `at`, at the start of a line.
fn line_at(at: u64) -> Resume {
    Resume { at, inside: false }
}

/// A read that starts at `at`, inside a line too long to read.
fn inside_at(at: u64) -> Resume {
    Resume { at, inside: true }
}

#[test]
fn a_line_cut_at_the_cap_or_still_being_written_is_read_again_from_its_start() {
    let root = scratch("cap");
    let path = root.join("t.jsonl");
    // Three lines of 20 bytes each, the last one not finished yet.
    let line = |n: i64| format!("{{\"n\":{n},\"p\":\"{}\"}}", "x".repeat(5));
    assert_eq!(line(1).len() + 1, 20);
    let text = format!("{}\n{}\n{}", line(1), line(2), line(3));
    write::write_file(&path, text.as_bytes()).unwrap();
    // A cap of 30 cuts the second line: it is read again next time, from its start.
    let read = read_ns(&path, Resume::default(), 30);
    assert_eq!(read, (vec![1], line_at(20), true));
    let read = read_ns(&path, read.1, 30);
    assert_eq!(read, (vec![2], line_at(40), true));
    // The third line has no end yet: nothing, and the same place again.
    let read = read_ns(&path, read.1, 30);
    assert_eq!(read, (vec![], line_at(40), false));
    append(&path, "\n");
    let read = read_ns(&path, read.1, 30);
    assert_eq!(read, (vec![3], line_at(60), false));
    // A file shorter than its mark was replaced: read from its start.
    write::write_file(&path, format!("{}\n", line(4)).as_bytes()).unwrap();
    assert_eq!(read_ns(&path, read.1, 30), (vec![4], line_at(20), false));
    assert!(read_on(&root.join("missing.jsonl"), Resume::default(), 30).is_err());
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn a_line_longer_than_the_cap_is_skipped_to_its_end() {
    let root = scratch("long");
    let path = root.join("t.jsonl");
    // A line of 115 bytes, then one of 8.
    let long = format!("{{\"n\":1,\"p\":\"{}\"}}", "x".repeat(100));
    let text = format!("{long}\n{{\"n\":2}}\n");
    assert_eq!(text.len(), 123);
    write::write_file(&path, text.as_bytes()).unwrap();
    let read = read_ns(&path, Resume::default(), 40);
    assert_eq!(read, (vec![], inside_at(40), true));
    let read = read_ns(&path, read.1, 40);
    assert_eq!(read, (vec![], inside_at(80), true));
    // The long line ends in this read, and the next is cut: it is read again from its start.
    let read = read_ns(&path, read.1, 40);
    assert_eq!(read, (vec![], line_at(115), true));
    let read = read_ns(&path, read.1, 40);
    assert_eq!(read, (vec![2], line_at(123), false));
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn a_file_read_by_two_steers_from_the_same_place_is_read_once_for_both() {
    let root = scratch("twice");
    let path = root.join("t.jsonl");
    let steer = user(STEER, "2026-11-11T10:00:04Z").to_string();
    write::write_file(&path, jsonl(&[&steer]).as_bytes()).unwrap();
    let place = Place {
        path,
        resume: Resume::default(),
    };
    let other = Wanted::new(["Read clause 9"].into_iter(), Timestamp::new(SENT).unwrap());
    let seen = taken(vec![(vec![place.clone()], wanted()), (vec![place], other)]);
    assert_eq!(
        seen.iter().map(|seen| seen.found).collect::<Vec<_>>(),
        [true, false]
    );
    assert_eq!(seen[0].places, seen[1].places);
    write::remove_dir_all(&root).unwrap();
}

//! The transcript matcher: a steer is found by an exact trimmed match in a line of a kind that
//! means taken, stamped no earlier than its send less the skew; other kinds, earlier lines and
//! files that cannot be read show nothing.

use std::path::PathBuf;

use clauseo_protocol::{AgentSessionId, Timestamp};
use serde_json::{Value, json};

use super::{Session, Wanted, read_lines, shows, taken};
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

/// A fresh folder under the temp folder for `test`.
fn scratch(test: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("clauseo-transcript-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

#[test]
fn a_transcript_is_read_from_the_session_file_and_unreadable_ones_show_nothing() {
    let root = scratch("session");
    let folder = root.join("matter");
    write::create_dir_all(&folder).unwrap();
    let home = root.join("home");
    let session = Session {
        home,
        folder,
        id: AgentSessionId::new("sess-1").unwrap(),
    };
    let asks = [(session.clone(), wanted())];
    // No file yet.
    assert_eq!(taken(&asks), [false]);
    let path = session.transcripts().remove(0);
    write::create_dir_all(path.parent().unwrap()).unwrap();
    // Not JSON lines, then a line of another kind: nothing.
    write::write_file(&path, b"not json\n{\"type\":\"assistant\"}\n").unwrap();
    assert_eq!(taken(&asks), [false]);
    let line = user(STEER, "2026-11-11T10:00:03Z").to_string();
    let mut text = "not json\n".to_owned();
    text.push_str(&line);
    text.push('\n');
    write::write_file(&path, text.as_bytes()).unwrap();
    assert_eq!(taken(&asks), [true]);
    // A session id that is not a plain file name is never read.
    let odd = Session {
        id: AgentSessionId::new("../sess-1").unwrap(),
        ..session
    };
    assert_eq!(taken(&[(odd, wanted())]), [false]);
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn only_the_tail_is_read_and_its_partial_first_line_is_skipped() {
    let root = scratch("tail");
    let path = root.join("t.jsonl");
    let early = user(STEER, "2026-11-11T10:00:03Z").to_string();
    let filler = json!({"type": "assistant", "pad": "x".repeat(300 * 1024)}).to_string();
    let late = user("Read clause 9", "2026-11-11T10:00:04Z").to_string();
    write::write_file(&path, format!("{early}\n{filler}\n{late}\n").as_bytes()).unwrap();
    // The steer's line is more than 256 KiB from the end; the filler line is cut, so skipped.
    let lines = read_lines(&path);
    assert_eq!(lines, [serde_json::from_str::<Value>(&late).unwrap()]);
    assert_eq!(read_lines(&root.join("missing.jsonl")), Vec::<Value>::new());
    write::remove_dir_all(&root).unwrap();
}

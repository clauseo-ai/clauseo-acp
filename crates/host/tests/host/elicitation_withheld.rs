//! Elicitation values withheld from tool output (0002, `elicitation_resolved`): a tool call
//! that echoes what the lawyer typed into a form logs `[withheld]` in its place, inline and in
//! a blob, and the store holds no byte of the value. What is not withheld is pinned too: another
//! call's echo under a scoped form, an echo after the call completed, declined and cancelled
//! forms, a short value of an unscoped form, and the next turn.

use std::fmt::Write as _;
use std::path::Path;

use clauseo_protocol::{
    BlobRef, CommandOutcome, ContentBlock, Count, DomainEvent, ElicitationId, ElicitationResponse,
    HostCommand, HostQuery, QueryResult, RawPayload, ReadLen, StopReason, StreamId, TextOrBlob,
    ThreadId, ToolCallContent, WITHHELD,
};
use serde_json::{Value, json};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

const SECRET: &str = "sk-test-SECRET-123";
const QUOTED: &str = "say \"hi\" C:\\dir";
const UNSCOPED: &str = "unscoped-SECRET-9";
const SIGNER: &str = "Müller-Geheim";

/// The `n`th elicitation the thread logged (from 1).
fn requested(events: &mut Events, n: usize) -> ElicitationId {
    let is = |event: &DomainEvent| matches!(event, DomainEvent::ElicitationRequested(_));
    events.until("ElicitationRequested", |e| e.count(is) >= n);
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ElicitationRequested(requested) => Some(requested.elicitation),
            _ => None,
        })
        .nth(n - 1)
        .unwrap()
}

fn answer(
    host: &Running,
    thread: ThreadId,
    elicitation: ElicitationId,
    response: ElicitationResponse,
) {
    assert_eq!(
        host.ok(HostCommand::AnswerElicitation {
            thread,
            elicitation,
            response,
        }),
        CommandOutcome::Done
    );
}

fn accept(values: Value) -> ElicitationResponse {
    ElicitationResponse::Accept {
        values: serde_json::from_value::<RawPayload>(values).unwrap(),
    }
}

/// What tool call `acp_id` logged, in order: each event's `raw_output` and its content texts.
fn logged(events: &Events, acp_id: &str) -> Vec<(Option<TextOrBlob>, Vec<String>)> {
    let id = events
        .events()
        .find_map(|event| match event {
            DomainEvent::ToolCallStarted(started) if started.acp_id == acp_id => {
                Some(started.tool_call)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no ToolCallStarted for {acp_id}"));
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ToolCallStarted(started) if started.tool_call == id => {
                Some((started.raw_output.clone(), texts(&started.content)))
            }
            DomainEvent::ToolCallUpdated(updated) if updated.tool_call == id => Some((
                updated.patch.raw_output.clone(),
                texts(updated.patch.content.as_deref().unwrap_or_default()),
            )),
            _ => None,
        })
        .collect()
}

fn texts(content: &[ToolCallContent]) -> Vec<String> {
    content
        .iter()
        .filter_map(|item| match item {
            ToolCallContent::Content {
                content: ContentBlock::Text { text },
            }
            | ToolCallContent::Text {
                text: TextOrBlob::Inline(text),
            } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// An inline JSON output, parsed.
fn inline_json(output: Option<&TextOrBlob>) -> Value {
    match output {
        Some(TextOrBlob::Inline(text)) => {
            serde_json::from_str(text).unwrap_or_else(|error| panic!("not JSON ({error}): {text}"))
        }
        other => panic!("not an inline output: {other:?}"),
    }
}

/// The whole text of a blob.
fn blob_text(host: &Running, output: Option<&TextOrBlob>) -> String {
    let Some(TextOrBlob::Blob(blob)) = output else {
        panic!("not a blob: {output:?}");
    };
    let blob: &BlobRef = blob;
    let query = HostQuery::ReadBlob {
        hash: blob.hash(),
        offset: Count::new(0).unwrap(),
        len: ReadLen::new(1024 * 1024).unwrap(),
    };
    match host.query(query) {
        Ok(QueryResult::Blob(range)) => {
            assert!(!range.text.contains(SECRET) && !range.text.contains(UNSCOPED));
            assert!(
                !blob.preview().contains(SECRET),
                "the preview holds the value"
            );
            range.text
        }
        other => panic!("ReadBlob answered {other:?}"),
    }
}

/// `value` as the store could hold it: raw, JSON-escaped (inside JSON text, which is how a
/// `raw_output` is stored), and JSON-escaped with non-ASCII chars as `\uXXXX` in either hex case.
fn forms(value: &str) -> Vec<String> {
    let quoted = serde_json::to_string(value).unwrap();
    let escaped = quoted[1..quoted.len() - 1].to_owned();
    let ascii = |upper: bool| {
        let mut out = String::new();
        for c in escaped.chars() {
            if c.is_ascii() {
                out.push(c);
            } else {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    if upper {
                        write!(out, "\\u{unit:04X}").unwrap();
                    } else {
                        write!(out, "\\u{unit:04x}").unwrap();
                    }
                }
            }
        }
        out
    };
    let mut all = vec![value.to_owned(), escaped.clone(), ascii(false), ascii(true)];
    all.dedup();
    all
}

/// Whether any form of `value` ([`forms`]) is stored under `dir`.
fn stored(dir: &Path, value: &str) -> bool {
    forms(value).iter().any(|form| stored_bytes(dir, form))
}

/// Every byte under the test's folder (the store's database and its WAL, blobs included, the
/// log file, settings): whether `needle` occurs in any file.
fn stored_bytes(dir: &Path, needle: &str) -> bool {
    let mut found = false;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found |= stored_bytes(&path, needle);
        } else if let Ok(bytes) = std::fs::read(&path) {
            found |= bytes
                .windows(needle.len())
                .any(|window| window == needle.as_bytes());
        }
    }
    found
}

/// A form scoped to the tool call: every string the lawyer submitted is withheld from the
/// call's later output, in JSON values and keys, in content text, in a blob, and after a quote
/// or a backslash; the store holds none of them.
#[test]
fn a_tool_call_echoing_its_form_values_logs_withheld() {
    let dir = Dir::new(
        "elicitation_echo_withheld",
        &[(FAKE, Scenario::Bundled("elicitation-echo-withheld"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Set up the search server.");
    let elicitation = requested(&mut events, 1);
    answer(
        &host,
        thread,
        elicitation,
        accept(
            json!({"server_code": SECRET, "note": "ok", "question_1": ["Fees"],
            "quoted": QUOTED, "signer": SIGNER, "pin": "12345678"}),
        ),
    );
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    events.assert_no_mismatch();

    let logged = logged(&events, "tc-cfg-1");
    // Started (before the answer), the echo, the large echo, the completion.
    assert_eq!(logged.len(), 4, "{logged:?}");
    // Every string of 3 chars or more withheld, the array's "Fees" included, escaped inside
    // JSON text too, and the PIN echoed as a number; "ok" is under 3 chars. The JSON stays valid.
    assert_eq!(
        inline_json(logged[1].0.as_ref()),
        json!({
            "echo": "configured with [withheld]",
            WITHHELD: 1,
            "note": "ok",
            "picked": [WITHHELD],
            "quoted": "got [withheld]",
            "nested": "{\"quoted\":\"[withheld]\"}",
            "pin": WITHHELD,
        })
    );
    assert_eq!(
        logged[1].1,
        [
            "Configured with [withheld], note ok, schedules [withheld].",
            "{\"signer\":\"[withheld]\"}",
        ]
    );
    // Over the inline limit: a blob, scrubbed before it was made.
    let blob = blob_text(&host, logged[2].0.as_ref());
    assert!(blob.contains("key [withheld] accepted."), "{blob}");

    host.shutdown();
    assert!(!stored(dir.home(), SECRET), "the store holds the API key");
    assert!(
        !stored(dir.home(), QUOTED),
        "the store holds the quoted value"
    );
    assert!(!stored(dir.home(), SIGNER), "the store holds the signer");
}

/// What a scoped form does not cover: another call's echo, the call's own echo after it
/// completed, and anything after a declined or a cancelled form.
#[test]
fn other_calls_later_output_and_unaccepted_forms_are_not_withheld() {
    let dir = Dir::new(
        "elicitation_echo_residuals",
        &[(FAKE, Scenario::Bundled("elicitation-echo-residuals"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Set up the search server.");
    let first = requested(&mut events, 1);
    answer(
        &host,
        thread,
        first,
        accept(json!({ "server_code": SECRET })),
    );
    let declined = requested(&mut events, 2);
    answer(&host, thread, declined, ElicitationResponse::Decline);
    let cancelled = requested(&mut events, 3);
    answer(&host, thread, cancelled, ElicitationResponse::Cancel);
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    events.assert_no_mismatch();

    // Another tool call repeating the value: a residual, pinned.
    let other = logged(&events, "tc-2");
    assert_eq!(
        inline_json(other[0].0.as_ref()),
        json!({ "other": format!("another call says {SECRET}") })
    );
    // The scoped call: its completing update is withheld, what comes after is not.
    let scoped = logged(&events, "tc-1");
    assert_eq!(
        inline_json(scoped[1].0.as_ref()),
        json!({"echo": "done with [withheld]"})
    );
    assert_eq!(
        inline_json(scoped[2].0.as_ref()),
        json!({ "after": format!("after completion {SECRET}") })
    );
    // Decline and Cancel keep nothing.
    let unanswered = logged(&events, "tc-3");
    assert_eq!(
        inline_json(unanswered[1].0.as_ref()),
        json!({"echo": "declined-SECRET-456 cancelled-SECRET-789"})
    );
    host.shutdown();
    // The scan finds a value the host did log.
    assert!(stored(dir.home(), SECRET));
}

/// A form with no tool call scope: a string of 8 chars or more is withheld from every later
/// tool call of the turn, inline and in a blob; a 7-char one is not; the next turn is not.
#[test]
fn an_unscoped_form_withholds_long_values_from_the_whole_turn() {
    let dir = Dir::new(
        "elicitation_echo_unscoped",
        &[(FAKE, Scenario::Bundled("elicitation-echo-unscoped"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Search the register.");
    let elicitation = requested(&mut events, 1);
    answer(
        &host,
        thread,
        elicitation,
        accept(json!({"register_code": UNSCOPED, "short": "seven77"})),
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);

    let call = logged(&events, "tc-a");
    assert_eq!(
        inline_json(call[1].0.as_ref()),
        json!({"echo": "token [withheld] code seven77"})
    );
    assert_eq!(call[1].1, ["Used [withheld] with code seven77."]);
    let blob = blob_text(&host, call[2].0.as_ref());
    assert!(blob.contains("token [withheld] ok."), "{blob}");
    host.settle();
    assert!(!stored(dir.home(), UNSCOPED), "the store holds the token");

    // The set went with the turn: the next turn's echo is logged as sent.
    let (second, _) = host.prompt(thread, "Search again.");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    let next = logged(&events, "tc-b");
    assert_eq!(
        inline_json(next[0].0.as_ref()),
        json!({ "echo": format!("next turn {UNSCOPED}") })
    );
    host.shutdown();
    assert!(stored(dir.home(), UNSCOPED));
}

/// The scan sees a value only stored escaped: the quoted value as a `raw_output` holds it, and
/// the signer as Python's `ensure_ascii` writes it. A scan for the raw bytes alone misses both.
#[test]
fn the_store_scan_finds_escaped_forms() {
    let dir = Dir::new("elicitation_scan_forms", &[]);
    let quoted = dir.data().join("quoted.bin");
    crate::write::write(&quoted, br#"{"quoted":"got say \"hi\" C:\\dir"}"#);
    assert!(!stored_bytes(dir.home(), QUOTED));
    assert!(stored(dir.home(), QUOTED));
    crate::write::remove_file(&quoted);
    crate::write::write(
        &dir.data().join("signer.bin"),
        br#"{"signer":"M\u00FCller-Geheim"}"#,
    );
    assert!(!stored_bytes(dir.home(), SIGNER));
    assert!(stored(dir.home(), SIGNER));
}

//! Reopening a conversation whose question is as long as Clauseo sends: the agent replays the
//! question as one `user_message_chunk` line, which must fit [`MAX_LINE_BYTES`]. Each fixture holds
//! a header line (`recorded`: whether the line after it is a live recording) and one replay line,
//! recorded from claude-agent-acp 0.89.1 and codex-acp 2.2.2, byte for byte but for the redacted
//! ids. The test swaps the line's question for
//! one of [`MAX_PROMPT_TEXT_BYTES`] escaped bytes, keeping every other byte as the adapter wrote
//! it, and reads the result through the line splitter.

use clauseo_protocol::{MAX_PROMPT_TEXT_BYTES, escaped_len};
use serde_json::Value;

use super::{MAX_LINE_BYTES, lines};

const FIXTURES: [(&str, &str); 2] = [
    (
        "claude-agent-acp",
        include_str!("fixtures/replay-claude-agent-acp.jsonl"),
    ),
    ("codex-acp", include_str!("fixtures/replay-codex-acp.jsonl")),
];

/// The fixture's header and its replay line, as written.
fn read(fixture: &'static str) -> (Value, &'static str) {
    let mut fixture_lines = fixture.lines();
    let header = serde_json::from_str(fixture_lines.next().unwrap()).unwrap();
    (header, fixture_lines.next().unwrap())
}

/// A question that costs exactly `MAX_PROMPT_TEXT_BYTES` escaped bytes, the worst case for raw
/// size per escaped byte aside: every character a 6-byte `\u0001` escape.
fn maximal_question() -> String {
    let text = "\u{1}".repeat(MAX_PROMPT_TEXT_BYTES / 6);
    assert_eq!(escaped_len(&text), MAX_PROMPT_TEXT_BYTES);
    text
}

#[test]
fn a_replayed_maximal_question_fits_the_line_cap() {
    let question = serde_json::to_string(&maximal_question()).unwrap();
    for (agent, fixture) in FIXTURES {
        let (header, line) = read(fixture);
        assert!(
            header["recorded"].is_boolean(),
            "{agent}: a header says recorded or not"
        );
        let parsed: Value = serde_json::from_str(line).unwrap();
        let update = &parsed["params"]["update"];
        assert_eq!(update["sessionUpdate"], "user_message_chunk", "{agent}");
        let original = update["content"]["text"].as_str().unwrap();
        let written = serde_json::to_string(original).unwrap();
        assert_eq!(header["recorded"], true, "{agent}: a live recording");
        // The adapter escapes as serde_json does (lowercase \u0001, raw UTF-8 for Devanagari and
        // emoji), so the line is its envelope plus our escaped measure of the text, quotes aside.
        assert_eq!(
            line.matches(written.as_str()).count(),
            1,
            "{agent}: the question once"
        );
        assert_eq!(written.len(), escaped_len(original) + 2, "{agent}");
        let envelope = line.len() - written.len();
        assert!(
            envelope + MAX_PROMPT_TEXT_BYTES + 2 <= MAX_LINE_BYTES,
            "{agent}: envelope {envelope} bytes"
        );
        let long = line.replacen(written.as_str(), &question, 1);
        assert!(
            long.len() <= MAX_LINE_BYTES,
            "{agent}: a replayed 6 MiB question is {} bytes, over {MAX_LINE_BYTES}",
            long.len()
        );
        let mut bytes = long.into_bytes();
        bytes.push(b'\n');
        let read = lines(bytes, 64 * 1024).unwrap_or_else(|_| panic!("{agent}: refused"));
        assert_eq!(read.len(), 1, "{agent}");
        let replayed: Value = serde_json::from_slice(&read[0]).unwrap();
        let text = replayed["params"]["update"]["content"]["text"]
            .as_str()
            .unwrap();
        assert_eq!(escaped_len(text), MAX_PROMPT_TEXT_BYTES, "{agent}");
    }
}

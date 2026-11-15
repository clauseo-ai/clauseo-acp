//! What the start of an oversize `session/update` says (`line/oversize.rs`, `SkippedUpdate`):
//! the session, the kind, and a tool call's id and status when they come before its content.

use super::{MAX_LINE_BYTES, SplitError, Trickle};
use crate::line::LineSplitter;
use crate::line::oversize::{SkippedUpdate, skipped_update};

const ENVELOPE: &str = r#"{"jsonrpc":"2.0","method":"session/update","params":"#;

/// What the start `params` (written after [`ENVELOPE`]) says.
fn read(params: &str) -> SkippedUpdate {
    skipped_update(format!("{ENVELOPE}{params}").as_bytes()).expect("a skippable start")
}

fn said(
    session: Option<&str>,
    tool_call_update: Option<bool>,
    tool_call: Option<&str>,
    status: Option<&'static str>,
) -> SkippedUpdate {
    SkippedUpdate {
        session: session.map(str::to_owned),
        tool_call_update,
        tool_call: tool_call.map(str::to_owned),
        status,
    }
}

#[test]
fn a_status_before_the_content_is_read() {
    // claude-agent-acp's order for a tool result, its `_meta` walked over.
    let claude = r#"{"sessionId":"s-1","update":{"_meta":{"claudeCode":{"toolName":"Read","x":[1,{"y":"}\"]"}]}},"toolCallId":"toolu_1","sessionUpdate":"tool_call_update","status":"failed","rawOutput":"AAAA"#;
    assert_eq!(
        read(claude),
        said(Some("s-1"), Some(true), Some("toolu_1"), Some("failed"))
    );
    // codex-acp's order.
    let codex = r#"{"sessionId":"s-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"call_1","kind":"execute","title":"ls","status":"completed","content":[{"type":"content","content":{"type":"text","text":"AAAA"#;
    assert_eq!(
        read(codex),
        said(Some("s-1"), Some(true), Some("call_1"), Some("completed"))
    );
    // Whitespace between the tokens, and every status.
    for status in ["pending", "in_progress", "completed", "failed"] {
        let spaced = format!(
            " {{ \"sessionId\" : \"s\" ,\n \"update\" :{{\t\"sessionUpdate\":\"tool_call_update\" , \"toolCallId\" : \"t\" , \"status\" : \"{status}\" , \"content\" : [\"AAAA"
        );
        assert_eq!(read(&spaced).status, Some(status), "{spaced}");
    }
}

#[test]
fn a_status_after_the_content_is_not_read() {
    let after = r#"{"sessionId":"s-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"t-1","content":[],"status":"failed"}}"#;
    assert_eq!(
        read(after),
        said(Some("s-1"), Some(true), Some("t-1"), None)
    );
    // A start that ends inside the content.
    let cut = r#"{"sessionId":"s-1","update":{"toolCallId":"t-1","sessionUpdate":"tool_call_update","rawOutput":{"text":"AAAA"#;
    assert_eq!(read(cut), said(Some("s-1"), Some(true), Some("t-1"), None));
}

#[test]
fn another_kind_is_read_as_such() {
    let chunk = r#"{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"AAAA"#;
    assert_eq!(read(chunk), said(Some("s-1"), Some(false), None, None));
}

#[test]
fn a_repeated_key_reads_its_last_value() {
    let twice = r#"{"sessionId":"a","sessionId":"b","update":{"sessionUpdate":"tool_call_update","toolCallId":"t-1","status":"completed","toolCallId":"t-2","status":"failed","content":"#;
    assert_eq!(
        read(twice),
        said(Some("b"), Some(true), Some("t-2"), Some("failed"))
    );
    // A later value it cannot read leaves the field unknown, not the earlier value.
    let unreadable = r#"{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t-1","status":"failed","status":"done","content":"#;
    assert_eq!(
        read(unreadable),
        said(Some("s"), Some(true), Some("t-1"), None)
    );
    let escaped = r#"{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t-1","toolCallId":"t\u002d2","status":"failed"}}"#;
    assert_eq!(read(escaped), said(Some("s"), Some(true), None, None));
}

#[test]
fn malformed_starts_read_nothing_more_and_never_panic() {
    let none = SkippedUpdate::default();
    for params in [
        "",
        "{",
        "[",
        "null",
        r#""s""#,
        r#"{"sessionId""#,
        r#"{"sessionId":"#,
        r#"{"sessionId":"s"#,
        r#"{"sessionId":5"#,
        r#"{"sessionId":null,"#,
        r#"{"session\u0049d":"s","update":{"toolCallId":"t","status":"failed"}}"#,
        r#"{"x":{"a":[}}}"#,
        r#"{"x":]"#,
        r#"{"x":"\"#,
        r#"{"x":"AAAA"#,
        r#"{"x":}"#,
        r#"{"x" "y"}"#,
        r#"{"update":"tool_call_update","sessionId":"#,
        r#"{"update":[{"toolCallId":"t","status":"failed"}]}"#,
        "{\"sessionId\":\"a\u{1}b\"}",
    ] {
        let got = read(params);
        assert!(
            got.tool_call.is_none() && got.status.is_none(),
            "{params}: {got:?}"
        );
        if !params.contains("sessionId\":\"s\"") {
            assert_eq!(got.session, none.session, "{params}");
        }
    }
    // A status that is not one of ACP's, or not a plain string.
    for status in [r#""done""#, "1", "null", r#""fail\u0065d""#, r#"{"a":1}"#] {
        let params = format!(
            r#"{{"sessionId":"s","update":{{"sessionUpdate":"tool_call_update","toolCallId":"t","status":{status}}}}}"#
        );
        assert_eq!(read(&params).status, None, "{params}");
    }
    // A tool call id that is not UTF-8 or holds a control character.
    let mut bytes =
        format!(r#"{ENVELOPE}{{"sessionId":"s","update":{{"toolCallId":""#).into_bytes();
    bytes.extend_from_slice(b"\xff\"}}");
    assert_eq!(skipped_update(&bytes).unwrap().tool_call, None);
    // Deep nesting is walked without recursion.
    let deep = format!(
        r#"{{"sessionId":"s","update":{{"_meta":{}{},"toolCallId":"t","status":"failed"}}}}"#,
        "[".repeat(1 << 20),
        "]".repeat(1 << 20)
    );
    assert_eq!(read(&deep).status, Some("failed"));
    let unclosed = format!(
        r#"{{"sessionId":"s","update":{{"_meta":{}"#,
        "[".repeat(1 << 20)
    );
    assert_eq!(read(&unclosed), said(Some("s"), None, None, None));
}

#[test]
fn the_splitter_carries_what_the_start_said() {
    let start = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"t-1","status":"failed","content":[{"type":"content","content":{"type":"text","text":""#;
    let end = r#""}}]}}}"#;
    let mut line = start.as_bytes().to_vec();
    line.resize(MAX_LINE_BYTES + 10 - end.len(), b'A');
    line.extend_from_slice(end.as_bytes());
    line.push(b'\n');
    for step in [usize::MAX / 2, 1024 * 1024, 64 * 1024 + 3] {
        let mut splitter = LineSplitter::new(Trickle {
            bytes: line.clone(),
            at: 0,
            step,
        });
        let got = futures::executor::block_on(splitter.next_line());
        let Err(SplitError::Skipped { bytes, update }) = got else {
            panic!("step {step}: {got:?}");
        };
        assert_eq!(bytes, MAX_LINE_BYTES + 10);
        assert_eq!(
            update,
            said(Some("s-1"), Some(true), Some("t-1"), Some("failed"))
        );
    }
}

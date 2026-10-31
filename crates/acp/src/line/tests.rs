use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::io::AsyncRead;

use super::*;

mod gate;
mod sink;

/// Hands out its bytes `step` at a time, as a pipe that delivers a little per read.
struct Trickle {
    bytes: Vec<u8>,
    at: usize,
    step: usize,
}

impl AsyncRead for Trickle {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let end = (this.at + this.step).min(this.bytes.len());
        let count = (end - this.at).min(buf.len());
        buf[..count].copy_from_slice(&this.bytes[this.at..this.at + count]);
        this.at += count;
        Poll::Ready(Ok(count))
    }
}

fn lines(bytes: Vec<u8>, step: usize) -> Result<Vec<Vec<u8>>, SplitError> {
    let mut splitter = LineSplitter::new(Trickle { bytes, at: 0, step });
    futures::executor::block_on(async {
        let mut lines = Vec::new();
        while let Some(line) = splitter.next_line().await? {
            lines.push(line);
        }
        Ok(lines)
    })
}

#[test]
fn line_split_across_reads_kept_whole() {
    // A 200 KiB line spans several fills of the 64 KiB buffer; small reads split it further.
    let long: Vec<u8> = (0..200 * 1024)
        .map(|i| b'a' + u8::try_from(i % 26).unwrap())
        .collect();
    let mut input = b"{\"a\":1}\n".to_vec();
    input.extend_from_slice(&long);
    input.push(b'\n');
    input.extend_from_slice(b"last without newline");
    for step in [1, 7, 4096, 64 * 1024, usize::MAX / 2] {
        let lines = lines(input.clone(), step).unwrap();
        assert_eq!(lines.len(), 3, "step {step}");
        assert_eq!(lines[0], b"{\"a\":1}");
        assert_eq!(lines[1], long, "step {step}");
        assert_eq!(lines[2], b"last without newline");
    }
}

#[test]
fn line_at_8_mib_passes_and_one_byte_more_fails() {
    let mut exact = vec![b'x'; MAX_LINE_BYTES];
    exact.push(b'\n');
    assert_eq!(lines(exact, 1024 * 1024).unwrap()[0].len(), MAX_LINE_BYTES);
    // Over the cap delivered in one read and in 1 MiB reads. Either way the line spans many fills
    // of the 64 KiB read buffer, so the cap is checked on the joined line.
    for step in [usize::MAX / 2, 1024 * 1024] {
        let mut over = vec![b'x'; MAX_LINE_BYTES + 1];
        over.push(b'\n');
        assert!(
            matches!(lines(over, step), Err(SplitError::TooLong)),
            "step {step}"
        );
    }
    // Over the cap with no newline at all.
    let endless = vec![b'x'; MAX_LINE_BYTES + 1];
    assert!(matches!(
        lines(endless, 1024 * 1024),
        Err(SplitError::TooLong)
    ));
}

#[test]
fn empty_input_has_no_lines() {
    assert_eq!(lines(Vec::new(), 1).unwrap(), Vec::<Vec<u8>>::new());
    assert_eq!(lines(b"\n\n".to_vec(), 1).unwrap(), [b"", b""]);
}

/// `(is_message, is_update)` for a line, or `None` when it does not decode.
fn classify(line: &str) -> Option<(bool, bool)> {
    let envelope = serde_json::from_str::<Envelope>(line).ok()?;
    Some((envelope.is_message(), envelope.is_update()))
}

#[test]
fn envelope_counts_only_what_the_handler_counts_off() {
    let update = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s"}}"#;
    assert_eq!(classify(update), Some((true, true)));
    for (line, expected) in [
        // Another notification, a request, a response: passed on, not counted.
        (
            r#"{"jsonrpc":"2.0","method":"elicitation/complete","params":{}}"#,
            Some((true, false)),
        ),
        (
            r#"{"jsonrpc":"2.0","id":1,"method":"session/update","params":{}}"#,
            Some((true, false)),
        ),
        (
            r#"{"jsonrpc":"2.0","id":null,"result":{}}"#,
            Some((true, false)),
        ),
        // `params` that the handler would not see as an object.
        (
            r#"{"jsonrpc":"2.0","method":"session/update","params":[]}"#,
            Some((true, false)),
        ),
        (
            r#"{"jsonrpc":"2.0","method":"session/update"}"#,
            Some((true, false)),
        ),
        // Another version, an `id` with no `result` or `error` or with both: the SDK would
        // answer each with an error.
        (
            r#"{"jsonrpc":"1.0","method":"session/update","params":{}}"#,
            Some((false, false)),
        ),
        (r#"{"jsonrpc":"2.0","id":1}"#, Some((false, false))),
        (
            r#"{"jsonrpc":"2.0","id":1,"result":{},"error":{}}"#,
            Some((false, false)),
        ),
        (
            r#"{"jsonrpc":"2.0","id":1,"method":"x","result":{}}"#,
            Some((false, false)),
        ),
        // An escaped key or value reads as its text.
        (
            r#"{"json\u0072pc":"2.0","method":"session\/update","params":{}}"#,
            Some((true, true)),
        ),
        // A repeated key counts as its last value, as in the SDK.
        (
            r#"{"jsonrpc":"2.0","method":"x","method":"session/update","params":{}}"#,
            Some((true, true)),
        ),
        // Not JSON-RPC: a log object, no `method` or `id`, a scalar, an array.
        (r#"{"level":"info","msg":"ready"}"#, Some((false, false))),
        (r#"{"jsonrpc":"2.0"}"#, Some((false, false))),
        ("42", None),
        (
            r#"[{"jsonrpc":"2.0","method":"session/update","params":{}}]"#,
            None,
        ),
    ] {
        assert_eq!(classify(line), expected, "{line}");
    }
    // The handler's side of the same rule.
    assert!(counted("session/update", &serde_json::json!({})));
    assert!(counted(
        MALFORMED_LINE_METHOD,
        &serde_json::json!({"bytes": 3})
    ));
    assert!(!counted("session/update", &serde_json::Value::Null));
    assert!(!counted("elicitation/complete", &serde_json::json!({})));
}

/// A `session/update` whose `params` holds `value` under `"v"`.
fn update_with(value: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s","v":{value}}}}}"#
    )
}

/// A value nested in `depth` arrays.
fn nested(depth: usize) -> String {
    format!("{}{}", "[".repeat(depth), "]".repeat(depth))
}

/// Whether the SDK's own parse of a line gives one message (and not a malformed frame).
fn sdk_reads_a_message(line: &str) -> bool {
    match agent_client_protocol::TransportFrame::parse_json(line) {
        agent_client_protocol::TransportFrame::Single(_) => true,
        agent_client_protocol::TransportFrame::Malformed { .. } => false,
        agent_client_protocol::TransportFrame::Batch(_) => panic!("a batch: {line}"),
    }
}

/// The filter passes a line exactly when the SDK's parse reads it as a message. Each list says
/// what the SDK does, so a change to the SDK in either direction fails here.
#[test]
fn envelope_passes_exactly_what_the_sdk_parse_reads() {
    // serde_json reads at most 127 levels; the line's object and `params` take two.
    let refused = [
        // The SDK's `Value` decode refuses these.
        update_with("1e400"),
        update_with("-1e400"),
        r#"{"jsonrpc":"2.0","id":1e400,"result":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"result":{"a":[1e400]}}"#.to_owned(),
        update_with(r#""\ud800""#),
        update_with(r#""\udc00""#),
        update_with(r#"{"\ud800":1}"#),
        update_with(&nested(126)),
        update_with(r#"{"$serde_json::private::RawValue":"x"}"#),
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"\u0024serde_json::private::RawValue":"x"}}"#.to_owned(),
        // Its `RawJsonRpcMessage` decode refuses these: a `method` that is not a string,
        r#"{"jsonrpc":"2.0","method":5,"params":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":null,"params":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":{},"params":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":["session/update"],"params":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":true,"params":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":7,"method":5,"params":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":7,"method":null}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"session/update","method":5,"params":{}}"#.to_owned(),
        // an `id` that is not `null`, a string or an `i64`,
        r#"{"jsonrpc":"2.0","id":{},"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":[],"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":true,"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1.5,"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1.0,"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":18446744073709551615,"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":9223372036854775808,"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"id":{},"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1.5,"result":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":{},"error":{"code":1,"message":"m"}}"#.to_owned(),
        // `params` that is not an object, an array or `null`,
        r#"{"jsonrpc":"2.0","method":"x","params":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"x","params":5}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"x","params":true}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":7,"method":"x","params":5}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"session/update","params":{},"params":5}"#.to_owned(),
        // an `error` that is not an object with an `i32` `code` and a string `message`.
        r#"{"jsonrpc":"2.0","id":1,"error":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":5}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":null}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":"x","message":"m"}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":1.5,"message":"m"}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":2147483648,"message":"m"}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":1,"message":5}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":1}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":1,"message":"m","code":"x"}}"#.to_owned(),
    ];
    for line in &refused {
        assert!(!sdk_reads_a_message(line), "the SDK reads {line}");
        assert!(!classify(line).is_some_and(|c| c.0), "{line}");
    }
    let passed = [
        update_with(&nested(125)),
        update_with("1.7976931348623157e308"),
        update_with("-9223372036854775809"),
        update_with("18446744073709551616"),
        update_with(r#""\ud83d\ude00 \u0000""#),
        // Only `raw_value` is on: serde_json's `Number` token is an ordinary key.
        update_with(r#"{"$serde_json::private::Number":"1"}"#),
        r#"{"jsonrpc":"2.0","method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"x","params":null}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"x","params":[1]}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"x","params":5,"params":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":null,"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":"a","method":"x","params":{}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":-9223372036854775808,"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":9223372036854775807,"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":{},"id":1,"method":"x"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"result":null}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"result":5}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-2147483648,"message":"m"}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":1,"message":"m","data":null,"x":[]}}"#
            .to_owned(),
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":"x","message":"m","code":1}}"#.to_owned(),
    ];
    for line in &passed {
        assert!(sdk_reads_a_message(line), "the SDK refuses {line}");
        assert_eq!(classify(line).map(|c| c.0), Some(true), "{line}");
    }
    // serde_json builds a `Value` with the `Number` token as a key: `arbitrary_precision` is off.
    let number =
        serde_json::from_str::<serde_json::Value>(r#"{"$serde_json::private::Number":"x"}"#);
    assert_eq!(
        number.ok(),
        Some(serde_json::json!({"$serde_json::private::Number": "x"}))
    );
    // The SDK reads these as messages, but not as the line says, so the filter refuses them: a
    // RawValue key whose text is JSON (here `params` becomes `[1]`), first key of `params` or of
    // the line, and an agent's line naming the filter's own marker method. The filter also
    // refuses the RawValue key in any other place, where the SDK reads it as an ordinary key.
    for line in [
        update_with(r#"{"a":1,"$serde_json::private::RawValue":"x"}"#),
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"$serde_json::private::RawValue":"[1]"}}"#.to_owned(),
        r#"{"$serde_json::private::RawValue":"{\"jsonrpc\":\"2.0\",\"method\":\"x\"}"}"#.to_owned(),
        format!(r#"{{"jsonrpc":"2.0","method":"{MALFORMED_LINE_METHOD}","params":{{"bytes":1}}}}"#),
    ] {
        assert!(sdk_reads_a_message(&line), "the SDK refuses {line}");
        assert!(!classify(&line).is_some_and(|c| c.0), "{line}");
    }
}

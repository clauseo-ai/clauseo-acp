use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::io::AsyncRead;

use super::*;

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
    // Over the cap within one buffer, and over it across reads.
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
        // `params` that the handler would not see as an object, or another version.
        (
            r#"{"jsonrpc":"2.0","method":"session/update","params":[]}"#,
            Some((true, false)),
        ),
        (
            r#"{"jsonrpc":"2.0","method":"session/update"}"#,
            Some((true, false)),
        ),
        (
            r#"{"jsonrpc":"1.0","method":"session/update","params":{}}"#,
            Some((true, false)),
        ),
        // An escaped key or value reads as its text.
        (
            r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#,
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

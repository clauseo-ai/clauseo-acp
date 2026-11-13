//! A line over [`MAX_LINE_BYTES`]: a `session/update` notification is dropped and the reader
//! goes on; anything else stays fatal (`line/oversize.rs`).

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::task::{Context, Poll};

use futures::{FutureExt, StreamExt};

use super::{MAX_LINE_BYTES, SplitError, Trickle, lines};
use crate::line::oversize::is_update_notification;
use crate::line::{LineSplitter, incoming};
use crate::state::Shared;

/// A line that opens with `start`, pads a string to `total` bytes and closes, so its byte
/// length is exactly `total`.
fn padded(start: &str, total: usize) -> Vec<u8> {
    let end = "\"}}}";
    let mut line = start.as_bytes().to_vec();
    line.resize(total - end.len(), b'x');
    line.extend_from_slice(end.as_bytes());
    assert_eq!(line.len(), total);
    line
}

/// A `session/update` of `total` bytes in claude-agent-acp's key order.
fn update(total: usize) -> Vec<u8> {
    padded(
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","content":""#,
        total,
    )
}

const NEXT: &[u8] = br#"{"jsonrpc":"2.0","id":1,"result":{}}"#;

/// `first`, a newline, then [`NEXT`] and a newline.
fn then_next(mut first: Vec<u8>) -> Vec<u8> {
    first.push(b'\n');
    first.extend_from_slice(NEXT);
    first.push(b'\n');
    first
}

/// Every line `bytes` gives, the skips as `Err(bytes)`, up to the end or the first fatal error.
fn read_all(bytes: Vec<u8>, step: usize) -> (Vec<Result<Vec<u8>, usize>>, Option<SplitError>) {
    let mut splitter = LineSplitter::new(Trickle { bytes, at: 0, step });
    futures::executor::block_on(async {
        let mut out = Vec::new();
        loop {
            match splitter.next_line().await {
                Ok(Some(line)) => out.push(Ok(line)),
                Ok(None) => return (out, None),
                Err(SplitError::Skipped { bytes }) => out.push(Err(bytes)),
                Err(fatal) => return (out, Some(fatal)),
            }
        }
    })
}

#[test]
fn oversize_update_is_skipped_and_the_next_line_read() {
    for step in [usize::MAX / 2, 1024 * 1024, 64 * 1024 + 3] {
        let (out, fatal) = read_all(then_next(update(MAX_LINE_BYTES + 1)), step);
        assert!(fatal.is_none(), "step {step}: {fatal:?}");
        assert_eq!(
            out,
            [Err(MAX_LINE_BYTES + 1), Ok(NEXT.to_vec())],
            "step {step}"
        );
    }
    // Far over the cap, and two in a row.
    let mut input = then_next(update(3 * MAX_LINE_BYTES));
    input.extend_from_slice(&then_next(update(MAX_LINE_BYTES + 7)));
    let (out, fatal) = read_all(input, 1024 * 1024);
    assert!(fatal.is_none());
    let next = Ok(NEXT.to_vec());
    let expected = [
        Err(3 * MAX_LINE_BYTES),
        next.clone(),
        Err(MAX_LINE_BYTES + 7),
        next,
    ];
    assert_eq!(out, expected);
    // The stream ends inside the line: the skip, then the end.
    let (out, fatal) = read_all(update(MAX_LINE_BYTES + 1), 1024 * 1024);
    assert!(fatal.is_none());
    assert_eq!(out, [Err(MAX_LINE_BYTES + 1)]);
}

#[test]
fn update_at_the_cap_still_passes_whole() {
    let line = update(MAX_LINE_BYTES);
    let (out, fatal) = read_all(then_next(line.clone()), 1024 * 1024);
    assert!(fatal.is_none());
    assert_eq!(out, [Ok(line), Ok(NEXT.to_vec())]);
}

#[test]
fn oversize_response_request_or_unknown_line_stays_fatal() {
    for start in [
        // A response, and a request naming the update method.
        r#"{"jsonrpc":"2.0","id":7,"result":{"content":""#,
        r#"{"jsonrpc":"2.0","id":7,"method":"session/update","params":{"x":""#,
        r#"{"id":7,"jsonrpc":"2.0","method":"session/update","params":{"x":""#,
        // No method before `params`, or no method at all.
        r#"{"jsonrpc":"2.0","params":{"x":""#,
        r#"{"jsonrpc":"2.0","result":{"x":""#,
        r"xxxxxxxx",
        // Another method.
        r#"{"jsonrpc":"2.0","method":"session/request_permission","params":{"x":""#,
    ] {
        for step in [usize::MAX / 2, 1024 * 1024] {
            let (out, fatal) = read_all(then_next(padded(start, MAX_LINE_BYTES + 1)), step);
            assert!(out.is_empty(), "{start}");
            assert!(
                matches!(fatal, Some(SplitError::TooLong)),
                "{start}: {fatal:?}"
            );
        }
    }
}

#[test]
fn only_a_start_showing_an_update_notification_is_skippable() {
    for (head, skippable) in [
        (
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"#,
            true,
        ),
        (r#"{"method":"session/update","params":"#, true),
        (
            " {\n \"jsonrpc\" : \"2.0\" ,\t\"method\" :  \"session/update\" , \"params\" :{",
            true,
        ),
        (
            r#"{"jsonrpc":"2.0","x":null,"y":-1.5e3,"z":true,"method":"session/update","params":{"#,
            true,
        ),
        (
            r#"{"method":"x","method":"session/update","params":{"#,
            true,
        ),
        // The last `method` wins, as in the SDK.
        (
            r#"{"method":"session/update","method":"x","params":{"#,
            false,
        ),
        // An `id` before `params`, even null.
        (
            r#"{"jsonrpc":"2.0","id":null,"method":"session/update","params":{"#,
            false,
        ),
        (
            r#"{"jsonrpc":"2.0","method":"session/update","id":"a","params":{"#,
            false,
        ),
        // An escape in a key or in the method: refused rather than decoded.
        (
            r#"{"\u0069d":1,"method":"session/update","params":{"#,
            false,
        ),
        (r#"{"method":"session\/update","params":{"#, false),
        (r#"{"method":"session/update","para\u006ds":{"#, false),
        // `params` first, `method` after it, or the method a prefix.
        (r#"{"params":{"#, false),
        (r#"{"method":"session/updates","params":{"#, false),
        (r#"{"method":"session/update"}"#, false),
        // A nested value before `params`, or a start that stops short.
        (
            r#"{"x":{"id":1},"method":"session/update","params":{"#,
            false,
        ),
        (r#"{"method":"session/update","#, false),
        (r#"{"method":"session/upd"#, false),
        (r#"[{"method":"session/update","params":{"#, false),
        ("", false),
    ] {
        assert_eq!(is_update_notification(head.as_bytes()), skippable, "{head}");
    }
    // Both adapters' recorded replays.
    for fixture in [
        include_str!("fixtures/replay-claude-agent-acp.jsonl"),
        include_str!("fixtures/replay-codex-acp.jsonl"),
    ] {
        let line = fixture.lines().nth(1).unwrap();
        assert!(is_update_notification(line.as_bytes()));
    }
}

/// While the rest of an oversize update is dropped, the splitter holds nothing of it: polled
/// with the stream paused 20 MiB into the line, it has let the start go.
#[test]
fn dropping_an_oversize_update_holds_no_memory() {
    let total = 40 * 1024 * 1024;
    let mut splitter = LineSplitter::new(Stall {
        inner: Trickle {
            bytes: then_next(update(total)),
            at: 0,
            step: 1024 * 1024,
        },
        pause_after: 20 * 1024 * 1024,
        paused: false,
    });
    let waker = futures::task::noop_waker();
    let poll = splitter
        .next_line()
        .boxed_local()
        .poll_unpin(&mut Context::from_waker(&waker));
    assert!(poll.is_pending());
    assert!(
        splitter.dropping.is_some(),
        "the splitter is dropping the line"
    );
    assert_eq!(splitter.held(), 0, "the start of the line is let go");
    let rest = futures::executor::block_on(async {
        let skipped = splitter.next_line().await;
        let next = splitter.next_line().await;
        (skipped, next)
    });
    assert!(matches!(rest.0, Err(SplitError::Skipped { bytes }) if bytes == total));
    assert_eq!(rest.1.unwrap(), Some(NEXT.to_vec()));
    assert_eq!(splitter.held(), 0);
}

/// A [`Trickle`] that stalls once, when it has handed out `pause_after` bytes.
struct Stall {
    inner: Trickle,
    pause_after: usize,
    paused: bool,
}

impl futures::io::AsyncRead for Stall {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if !this.paused && this.inner.at >= this.pause_after {
            this.paused = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        std::pin::Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

/// [`incoming`] over `bytes`: the lines it passes on, and whether the stream ended in an error.
fn through_incoming(bytes: Vec<u8>) -> (Vec<String>, bool, Option<crate::Error>) {
    let (events, _events_rx) = async_channel::unbounded();
    let (stop, _stop_rx) = async_channel::unbounded();
    let shared = Arc::new(Shared::new(events, None, Arc::new(AtomicU64::new(0)), stop));
    let stream = incoming(
        Trickle {
            bytes,
            at: 0,
            step: 1024 * 1024,
        },
        Arc::clone(&shared),
    );
    let items: Vec<_> = futures::executor::block_on(stream.collect());
    let failed = items.iter().any(Result::is_err);
    let passed = items.into_iter().filter_map(Result::ok).collect();
    (passed, failed, shared.fatal())
}

/// The connection's view: an oversize update is dropped with no fatal error and the line after
/// it passed on; an oversize response ends the stream with `Error::Protocol`.
#[test]
fn incoming_keeps_the_connection_past_an_oversize_update_only() {
    let (passed, failed, fatal) = through_incoming(then_next(update(MAX_LINE_BYTES + 1)));
    assert_eq!(passed, [String::from_utf8(NEXT.to_vec()).unwrap()]);
    assert!(!failed && fatal.is_none(), "{fatal:?}");

    let response = padded(
        r#"{"jsonrpc":"2.0","id":7,"result":{"content":""#,
        MAX_LINE_BYTES + 1,
    );
    let (passed, failed, fatal) = through_incoming(then_next(response));
    assert_eq!(passed, Vec::<String>::new());
    assert!(failed);
    assert!(
        matches!(fatal, Some(crate::Error::Protocol(_))),
        "{fatal:?}"
    );
}

/// [`lines`] still reads a plain oversize line as fatal (the cap test's helper).
#[test]
fn plain_oversize_line_is_fatal_through_lines() {
    let over = then_next(vec![b'x'; MAX_LINE_BYTES + 1]);
    assert!(matches!(lines(over, 1024 * 1024), Err(SplitError::TooLong)));
}

//! The handshake's refusals: the reply's shape, and the deadline for a relay that never finishes.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::super::tests::{Scratch, TOKEN, good_env, relay, run_mode, token_file};

/// Waits for `after`, without `thread::sleep` (`clippy.toml` bans it). The channel's sender is
/// held, so nothing wakes the wait early.
fn pause(after: Duration) {
    let (_hold, wake) = mpsc::channel::<()>();
    if wake.recv_timeout(after).is_err() {
        // The time is up, as the sender is still held.
    }
}

/// The request line the tests send, with id `id`.
fn request(id: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"ping\"}}\n")
}

#[test]
fn a_handshake_reply_must_be_version_1_with_ok_true() {
    // Each is a refusal: no line is forwarded, and the request gets `connect_failed`.
    let refused = [
        r#"{"ok":true}"#,
        r#"{"clauseo_relay":2,"ok":true}"#,
        r#"{"clauseo_relay":"1","ok":true}"#,
        r#"{"clauseo_relay":1}"#,
        r#"{"clauseo_relay":1,"ok":"true"}"#,
        r#"{"clauseo_relay":1,"ok":false}"#,
    ];
    for answer in refused {
        let scratch = Scratch::new("strict-reply");
        let file = token_file(&scratch, TOKEN);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = relay(listener, Some(answer), |reader, _| {
            let mut rest = String::new();
            reader.read_line(&mut rest).unwrap();
            rest
        });
        let (result, capture) = run_mode(
            &good_env(port, &file),
            &["case-law"],
            request("\"x\"").as_bytes(),
        );
        assert_eq!(result.unwrap(), 0);
        let (_, forwarded) = server.join().unwrap();
        assert_eq!(forwarded, "", "{answer}: a refused link forwarded a line");
        let replies = capture.replies();
        assert_eq!(replies.len(), 1, "{answer}: {replies:?}");
        assert_eq!(
            replies[0]["error"]["data"]["reason"], "connect_failed",
            "{answer}"
        );
    }
}

#[test]
fn a_relay_that_trickles_bytes_past_the_deadline_is_given_up() {
    let scratch = Scratch::new("trickle");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The relay answers the hello with one byte a second and never a newline, for 30 seconds
    // unless the mode gives up first. It counts the bytes that were written.
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        let mut writer = &stream;
        let mut written = 0;
        for _ in 0..30 {
            if writer.write_all(b"x").is_err() {
                break;
            }
            written += 1;
            pause(Duration::from_secs(1));
        }
        written
    });
    let (result, capture) = run_mode(
        &good_env(port, &file),
        &["case-law"],
        request("1").as_bytes(),
    );
    assert_eq!(result.unwrap(), 0);
    let written = server.join().unwrap();
    // The deadline is 5 seconds: a few bytes went out before the mode gave up, not all 30.
    assert!(written < 10, "the handshake ran for {written} seconds");
    let replies = capture.replies();
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0]["id"], Value::from(1));
    assert_eq!(replies[0]["error"]["data"]["reason"], "connect_failed");
}

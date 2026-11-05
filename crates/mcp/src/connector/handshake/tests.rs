//! The handshake's refusals: the reply's shape, and the deadline for a relay that never finishes.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use serde_json::Value;

use super::super::tests::{
    Scratch, TOKEN, accept_within_limit, good_env, pause, relay, run_mode, token_file,
};

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

/// How long the relay keeps trickling: 10 one-second writes, then it stops on its own.
const TRICKLE_SECS: usize = 10;

/// What the trickling relay saw, named in the test's failure message.
#[derive(Debug)]
enum Trickle {
    /// The mode never connected, so the relay gave up waiting in `accept`.
    NeverConnected,
    /// Every write succeeded for the whole cap: the mode did not close the connection.
    WritesKeptSucceeding(usize),
    /// A write failed after this many bytes: the mode closed the connection.
    WriteFailed(usize),
}

#[test]
fn a_relay_that_trickles_bytes_past_the_deadline_is_given_up() {
    let scratch = Scratch::new("trickle");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The relay answers the hello with one byte a second and never a newline. It stops after
    // `TRICKLE_SECS` writes even if they all succeed, so the thread ends within about 10 s.
    let server = thread::spawn(move || {
        let Some(stream) = accept_within_limit(&listener) else {
            return Trickle::NeverConnected;
        };
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        let mut writer = &stream;
        for written in 0..TRICKLE_SECS {
            if writer.write_all(b"x").is_err() {
                return Trickle::WriteFailed(written);
            }
            pause(Duration::from_secs(1));
        }
        Trickle::WritesKeptSucceeding(TRICKLE_SECS)
    });
    let (result, capture) = run_mode(
        &good_env(port, &file),
        &["case-law"],
        request("1").as_bytes(),
    );
    // Checked before the relay is joined: a mode that stopped early leaves the relay waiting in
    // `accept`, and the reply is what says why.
    let replies = capture.replies();
    let deadline_reply =
        replies.len() == 1 && replies[0]["error"]["data"]["reason"] == "connect_failed";
    assert!(
        matches!(result, Ok(0)) && deadline_reply,
        "the mode did not stop at the deadline: result {result:?}, replies {replies:?}"
    );
    let written = match server.join().unwrap() {
        Trickle::WriteFailed(written) => written,
        Trickle::NeverConnected => {
            panic!(
                "the relay never connected, so the mode did not reach the host: replies {replies:?}"
            )
        }
        Trickle::WritesKeptSucceeding(bytes) => panic!(
            "the mode kept the connection open: {bytes} writes succeeded past the {TRICKLE_SECS} s cap; replies {replies:?}"
        ),
    };
    // The deadline is 5 seconds: the write fails before the 10-second cap, not after it.
    assert!(
        written < TRICKLE_SECS,
        "the handshake ran for {written} seconds"
    );
    assert_eq!(replies[0]["id"], Value::from(1));
}

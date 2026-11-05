//! The requests in flight when the link ends, and the cap on them. Each test runs the mode with
//! a relay thread standing in for the host, and stdin paced so that the order of events is fixed.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::line::Out;
use super::run_with;
use super::tests::{
    Capture, OK, Scratch, TOKEN, env_of, good_env, lines_until_end, relay, token_file,
};

/// Stdin that yields `first`, then waits until `open` fires, then yields `rest`.
struct Gated {
    first: Cursor<Vec<u8>>,
    rest: Cursor<Vec<u8>>,
    open: Option<Receiver<()>>,
}

impl Read for Gated {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.first.read(buf)?;
        if read > 0 {
            return Ok(read);
        }
        if let Some(open) = self.open.take() {
            // Dropped without a send, the gate opens too: nothing is left to wait for.
            if open.recv().is_err() {}
        }
        self.rest.read(buf)
    }
}

/// A writer that fires `tx` once, when a write holds `needle`. The bytes go to `out` as well.
struct Signal {
    out: Capture,
    needle: &'static str,
    sent: bool,
    tx: Sender<()>,
}

impl Write for Signal {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.out.write(buf)?;
        if !self.sent && String::from_utf8_lossy(buf).contains(self.needle) {
            self.sent = true;
            if self.tx.send(()).is_err() {
                // The gate is gone: the test ended before the reply.
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

fn line(id: u32) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{}}}}\n")
}

fn not_reachable(replies: &[Value], id: &Value) -> bool {
    replies.iter().any(|reply| {
        reply["id"] == *id
            && reply["error"]["message"]
                .as_str()
                .is_some_and(|text| text.contains("not reachable"))
            && reply["error"]["data"]["reason"] == "connect_failed"
    })
}

#[test]
fn a_request_in_flight_when_the_relay_drops_gets_not_reachable() {
    let scratch = Scratch::new("in-flight");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The relay reads request 4 and closes without a reply.
    let server = relay(listener, Some(OK), |reader, _| {
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        request
    });
    let (open_tx, open_rx) = mpsc::channel::<()>();
    let capture = Capture::default();
    let out: Out = Arc::new(Mutex::new(Signal {
        out: capture.clone(),
        needle: "\"id\":4",
        sent: false,
        tx: open_tx,
    }));
    // Request 6 is read only after request 4's answer has been written, so after the relay thread
    // has closed the pending set: it meets a dead link, and the socket may still take its bytes
    // (macOS, where the local shutdown of a reset socket fails), so it must be answered here.
    let stdin = BufReader::new(Gated {
        first: Cursor::new(line(4).into_bytes()),
        rest: Cursor::new(line(6).into_bytes()),
        open: Some(open_rx),
    });
    let vars = good_env(port, &file);
    let map = env_of(&vars);
    let lookup = |key: &str| map.get(key).cloned();
    let result = run_with(&[OsString::from("case-law")], &lookup, stdin, &out);
    assert_eq!(result.unwrap(), 0);
    let (_, forwarded) = server.join().unwrap();
    assert_eq!(forwarded, line(4));
    let replies = capture.replies();
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(not_reachable(&replies, &Value::from(4)), "{replies:?}");
    assert!(not_reachable(&replies, &Value::from(6)), "{replies:?}");
    assert!(!capture.text().contains(TOKEN));
}

#[test]
fn a_request_pending_at_stdin_end_is_answered_when_the_relay_outstays_the_grace() {
    let scratch = Scratch::new("pending-at-eof");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The relay reads request 4, then neither answers nor closes until the mode has returned.
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let server = relay(listener, Some(OK), move |reader, _| {
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        if release_rx.recv().is_err() {
            // The sender's drop, after the mode returned, is the release.
        }
        request
    });
    let capture = Capture::default();
    let out = capture.out();
    let vars = good_env(port, &file);
    let map = env_of(&vars);
    let lookup = |key: &str| map.get(key).cloned();
    let result = run_with(
        &[OsString::from("case-law")],
        &lookup,
        Cursor::new(line(4).into_bytes()),
        &out,
    );
    assert_eq!(result.unwrap(), 0);
    drop(release_tx);
    let (_, forwarded) = server.join().unwrap();
    assert_eq!(forwarded, line(4));
    let replies = capture.replies();
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(not_reachable(&replies, &Value::from(4)), "{replies:?}");
}

#[test]
fn the_cap_answers_the_65th_request_at_once_and_forwards_none_of_it() {
    let scratch = Scratch::new("cap-pending");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The relay takes lines and never answers; it sees the end when stdin closes.
    let server = relay(listener, Some(OK), |reader, _| lines_until_end(reader));
    let input: String = (1..=65).map(line).collect();
    let capture = Capture::default();
    let out = capture.out();
    let vars = good_env(port, &file);
    let map = env_of(&vars);
    let lookup = |key: &str| map.get(key).cloned();
    let result = run_with(
        &[OsString::from("case-law")],
        &lookup,
        Cursor::new(input.into_bytes()),
        &out,
    );
    assert_eq!(result.unwrap(), 0);
    let (_, forwarded) = server.join().unwrap();
    assert_eq!(forwarded.len(), 64);
    assert_eq!(forwarded[63], line(64));
    let replies = capture.replies();
    assert_eq!(replies.len(), 65, "{replies:?}");
    // The 65th came back first, while the first 64 were still in flight, and says the relay is
    // busy, not gone.
    assert_eq!(replies[0]["id"], 65);
    assert_eq!(replies[0]["error"]["data"]["reason"], "busy");
    assert!(
        replies[0]["error"]["message"]
            .as_str()
            .is_some_and(|text| text.contains("busy")),
        "{replies:?}"
    );
    for id in 1..=64 {
        assert!(not_reachable(&replies, &Value::from(id)), "{id}");
    }
    assert!(!capture.text().contains(TOKEN));
}

#[test]
fn an_answered_request_is_not_answered_again_when_the_relay_drops() {
    let scratch = Scratch::new("answered");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The relay replies to request 4, then closes.
    let server = relay(listener, Some(OK), |reader, writer| {
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        let mut writer = writer;
        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{}}\n")
            .unwrap();
        request
    });
    let capture = Capture::default();
    let out = capture.out();
    let vars = good_env(port, &file);
    let map = env_of(&vars);
    let lookup = |key: &str| map.get(key).cloned();
    let result = run_with(
        &[OsString::from("case-law")],
        &lookup,
        Cursor::new(line(4).into_bytes()),
        &out,
    );
    assert_eq!(result.unwrap(), 0);
    server.join().unwrap();
    let replies = capture.replies();
    assert_eq!(
        replies,
        [serde_json::json!({"jsonrpc":"2.0","id":4,"result":{}})]
    );
}

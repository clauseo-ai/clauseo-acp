//! The mode with a `shutdown` that does nothing: the Windows model. On Windows a `shutdown` does
//! not wake a `recv` blocked in another thread, so a design that ends its reader with one stalls
//! there until the peer acts. These tests give the mode a `shutdown` that neither wakes a reader
//! nor reaches the peer, so the stall shows on Linux too: the link must still end, the relay must
//! still see the end, and every request must still be answered once.

use std::ffi::OsString;
use std::io::{BufReader, Cursor};
use std::net::TcpListener;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::in_flight::{Gated, Signal};
use super::line::Out;
use super::tests::{
    Capture, OK, Scratch, TOKEN, assert_no_token, env_of, good_env, lines_until_end, relay,
    token_file,
};
use super::{MAX_LINE_BYTES, Shut, run_with_shut};

/// A `shutdown` that does nothing: it wakes no reader and sends the peer nothing.
pub(super) const IGNORED: Shut = |_socket, _how| Ok(());

fn request(id: u32) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\"}}\n")
}

fn is_connect_failed(reply: &Value, id: u32) -> bool {
    reply["id"] == id && reply["error"]["data"]["reason"] == "connect_failed"
}

#[test]
fn the_relay_sees_the_end_after_stdin_closes_when_the_half_close_does_nothing() {
    let scratch = Scratch::new("no-half-close");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // The relay never answers; it reads until the mode closes, within its 10 s bound.
    let server = relay(listener, Some(OK), |reader, _| lines_until_end(reader));
    let capture = Capture::default();
    let vars = good_env(port, &file);
    let map = env_of(&vars);
    let lookup = |key: &str| map.get(key).cloned();
    let result = run_with_shut(
        &[OsString::from("case-law")],
        &lookup,
        Cursor::new(request(1).into_bytes()),
        &capture.out(),
        IGNORED,
    );
    assert_eq!(result.unwrap(), 0);
    // A mode that leaves its socket open fails here, with the relay's read bound.
    let (_, seen) = server.join().unwrap();
    assert_eq!(seen, [request(1)]);
    let replies = capture.replies();
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(is_connect_failed(&replies[0], 1), "{replies:?}");
    assert_no_token(&capture);
}

#[test]
fn a_hang_up_ends_the_link_at_once_when_shutdown_wakes_no_reader() {
    let scratch = Scratch::new("no-wake");
    let file = token_file(&scratch, TOKEN);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = relay(listener, Some(OK), |reader, _| lines_until_end(reader));
    // Request 1 is forwarded, then a line over the cap ends the link. Request 2 is read only
    // after request 1's answer, which the link's end gives: stdin is still open, so nothing but
    // the hang-up can end the link.
    let mut first = request(1).into_bytes();
    first.extend(vec![b'x'; MAX_LINE_BYTES + 1]);
    first.push(b'\n');
    let (open_tx, open_rx) = mpsc::channel::<()>();
    let capture = Capture::default();
    let out: Out = Arc::new(Mutex::new(Signal::new(
        capture.clone(),
        "\"id\":1",
        open_tx,
    )));
    let stdin = BufReader::new(Gated::new(first, request(2).into_bytes(), open_rx));
    let vars = good_env(port, &file);
    let map = env_of(&vars);
    let lookup = |key: &str| map.get(key).cloned();
    let result = run_with_shut(&[OsString::from("case-law")], &lookup, stdin, &out, IGNORED);
    assert_eq!(result.unwrap(), 0);
    let (_, seen) = server.join().unwrap();
    assert_eq!(seen, [request(1)]);
    let replies = capture.replies();
    assert_eq!(replies.len(), 3, "{replies:?}");
    assert_eq!(replies[0]["error"]["code"], -32600, "{replies:?}");
    assert!(is_connect_failed(&replies[1], 1), "{replies:?}");
    assert!(is_connect_failed(&replies[2], 2), "{replies:?}");
    assert_no_token(&capture);
}

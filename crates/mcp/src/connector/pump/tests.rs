//! A request that comes after the link's end has been handled.

use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, mpsc};

use serde_json::{Value, json};

use super::super::tests::Capture;
use super::{Pending, Writer, send_request};

/// The relay thread has answered what was pending and closed the set, but the socket still takes
/// writes, as a socket whose peer has closed does until the peer's reset arrives (and as macOS
/// leaves it when the local shutdown fails with `ENOTCONN`). The request must be answered here
/// and not forwarded: no one is left to answer it.
#[test]
fn a_request_after_the_link_ended_is_answered_at_once_and_not_forwarded() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    let pending = Pending::default();
    assert_eq!(pending.close(), Vec::<Value>::new());
    let capture = Capture::default();
    let out = capture.out();
    let (stop, _stopped) = mpsc::channel();
    let mut writer = Some(Writer {
        socket: Arc::new(socket),
        stop,
    });
    let id = json!(6);
    send_request(
        &mut writer,
        &out,
        &pending,
        super::CONNECT_FAILED,
        &id,
        br#"{"jsonrpc":"2.0","id":6,"method":"tools/call"}"#,
    )
    .unwrap();
    assert!(writer.is_none(), "the link is dropped");
    let replies = capture.replies();
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0]["id"], 6);
    assert_eq!(replies[0]["error"]["data"]["reason"], "connect_failed");
    // The link was closed, by the drop of its only handle, with nothing written into it.
    let mut forwarded = String::new();
    assert_eq!(
        BufReader::new(peer).read_line(&mut forwarded).unwrap(),
        0,
        "{forwarded:?}"
    );
}

/// A forwarded request nudges the relay thread's reader, so a reply is not left waiting out the
/// reader's longest idle wait (20 ms, about 31 ms on Windows's 15.6 ms timer tick).
#[test]
fn a_forwarded_request_nudges_the_reader() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    let pending = Pending::default();
    let capture = Capture::default();
    let out = capture.out();
    let (stop, stopped) = mpsc::channel();
    let mut writer = Some(Writer {
        socket: Arc::new(socket),
        stop,
    });
    let line = br#"{"jsonrpc":"2.0","id":7,"method":"tools/call"}"#;
    send_request(
        &mut writer,
        &out,
        &pending,
        super::CONNECT_FAILED,
        &json!(7),
        line,
    )
    .unwrap();
    assert_eq!(
        stopped.try_recv(),
        Ok(()),
        "the request went out with no nudge"
    );
    let mut forwarded = String::new();
    BufReader::new(peer).read_line(&mut forwarded).unwrap();
    assert_eq!(forwarded.trim_end().as_bytes(), line);
    assert_eq!(
        capture.text(),
        "",
        "a forwarded request is not answered here"
    );
}

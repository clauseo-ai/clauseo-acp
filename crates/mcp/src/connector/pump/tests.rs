//! A request that comes after the link's end has been handled.

use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};

use serde_json::{Value, json};

use super::super::tests::Capture;
use super::{Pending, send_request};

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
    let mut writer = Some(socket);
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
    // The link was hung up with nothing written into it.
    let mut forwarded = String::new();
    assert_eq!(
        BufReader::new(peer).read_line(&mut forwarded).unwrap(),
        0,
        "{forwarded:?}"
    );
}

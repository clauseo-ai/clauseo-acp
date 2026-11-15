//! The relay: its handshake, framing, NUL splitting and routing, and the whole relay over a real
//! loopback socket with an in-memory pipe standing in for the browser.

use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use super::frames::{self, Message};
use super::handshake::{self, Verdict};
use super::routes::{FromBrowser, FromClient, Routes, ToGuard};
use super::seam::{FIRST_ID, is_guard_id, next_guard_id};
use super::{Devtools, nul};

const SECRET: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
/// RFC 6455 section 1.3's example key and its accept value.
const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
const ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";

fn head(target: &str, extra: &str) -> String {
    format!(
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:50000\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: {KEY}\r\nSec-WebSocket-Version: 13\r\n{extra}\r\n"
    )
}

#[test]
fn sha1_and_base64_match_their_test_vectors() {
    let hex = |bytes: &[u8]| {
        bytes.iter().fold(String::new(), |mut text, b| {
            write!(text, "{b:02x}").unwrap();
            text
        })
    };
    assert_eq!(
        hex(&handshake::sha1(b"")),
        "da39a3ee5e6b4b0d3255bfef95601890afd80709"
    );
    assert_eq!(
        hex(&handshake::sha1(b"abc")),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    assert_eq!(
        hex(&handshake::sha1(long)),
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
    );
    assert_eq!(handshake::base64(b""), "");
    assert_eq!(handshake::base64(b"f"), "Zg==");
    assert_eq!(handshake::base64(b"foob"), "Zm9vYg==");
    assert_eq!(handshake::base64(b"foobar"), "Zm9vYmFy");
}

#[test]
fn the_secret_is_256_bits_of_hex_and_new_each_time() {
    let one = handshake::new_secret().unwrap();
    assert_eq!(one.len(), 64);
    assert!(
        one.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_ne!(one, handshake::new_secret().unwrap());
}

#[test]
fn the_right_path_gets_the_upgrade() {
    let target = format!("/devtools/browser/{SECRET}");
    let Verdict::Upgrade(reply) = handshake::judge(head(&target, "").as_bytes(), 50_000, SECRET)
    else {
        panic!("refused the right path");
    };
    let reply = String::from_utf8(reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 101 "), "{reply}");
    assert!(
        reply.contains(&format!("Sec-WebSocket-Accept: {ACCEPT}\r\n")),
        "{reply}"
    );
}

#[test]
fn a_wrong_path_no_path_or_json_gets_404() {
    let wrong_secret = format!("/devtools/browser/{}", SECRET.replace('0', "1"));
    let short = format!("/devtools/browser/{}", &SECRET[..63]);
    let query = format!("/devtools/browser/{SECRET}?x=1");
    let page = format!("/devtools/page/{SECRET}");
    for target in [
        "/",
        "",
        "/json",
        "/json/version",
        "/json/list",
        "/devtools/browser/",
        wrong_secret.as_str(),
        short.as_str(),
        query.as_str(),
        page.as_str(),
    ] {
        let verdict = handshake::judge(head(target, "").as_bytes(), 50_000, SECRET);
        assert_eq!(verdict, Verdict::NotFound, "{target:?}");
    }
}

#[test]
fn a_page_origin_another_host_or_a_plain_get_gets_404() {
    let target = format!("/devtools/browser/{SECRET}");
    let refused = [
        head(&target, "Origin: http://example.com\r\n"),
        head(&target, "Host: 127.0.0.1:50000\r\n"),
        head(&target, "").replace("Host: 127.0.0.1:50000", "Host: localhost:50000"),
        head(&target, "").replace("Host: 127.0.0.1:50000", "Host: evil.example:50000"),
        head(&target, "").replace("Upgrade: websocket\r\n", ""),
        head(&target, "").replace("Version: 13", "Version: 8"),
        head(&target, "").replace("GET ", "POST "),
        format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:50000\r\n\r\n"),
    ];
    for request in refused {
        let verdict = handshake::judge(request.as_bytes(), 50_000, SECRET);
        assert_eq!(verdict, Verdict::NotFound, "{request}");
    }
    let other_port = handshake::judge(head(&target, "").as_bytes(), 50_001, SECRET);
    assert_eq!(other_port, Verdict::NotFound);
}

/// Hands out its bytes a few at a time, as a socket or a pipe may.
struct Trickle {
    bytes: Vec<u8>,
    at: usize,
    step: usize,
}

impl Read for Trickle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let end = (self.at + self.step).min(self.bytes.len());
        let take = (end - self.at).min(buf.len());
        buf[..take].copy_from_slice(&self.bytes[self.at..self.at + take]);
        self.at += take;
        Ok(take)
    }
}

fn trickle(bytes: Vec<u8>, step: usize) -> Trickle {
    Trickle { bytes, at: 0, step }
}

/// A masked client frame.
fn client_frame(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mask = [0x37_u8, 0xFA, 0x21, 0x3D];
    let mut frame = vec![if fin { 0x80 } else { 0 } | opcode];
    match payload.len() {
        len @ 0..=125 => frame.push(0x80 | u8::try_from(len).unwrap()),
        len @ 126..=0xFFFF => {
            frame.push(0x80 | 0x7E);
            frame.extend_from_slice(&u16::try_from(len).unwrap().to_be_bytes());
        }
        len => {
            frame.push(0x80 | 0x7F);
            frame.extend_from_slice(&u64::try_from(len).unwrap().to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    frame
}

#[test]
fn frames_come_whole_from_one_byte_reads_with_pings_between_fragments() {
    let big = "x".repeat(70_000);
    let mut bytes = client_frame(true, 0x1, br#"{"id":1}"#);
    bytes.extend(client_frame(false, 0x1, b"{\"id\":"));
    bytes.extend(client_frame(true, 0x9, b"hi"));
    bytes.extend(client_frame(true, 0x0, b"2}"));
    bytes.extend(client_frame(true, 0x2, big.as_bytes()));
    bytes.extend(client_frame(true, 0x8, b""));
    let mut reader = frames::Reader::new(trickle(bytes, 1));
    assert_eq!(
        reader.next().unwrap(),
        Message::Text(r#"{"id":1}"#.to_owned())
    );
    assert_eq!(reader.next().unwrap(), Message::Ping(b"hi".to_vec()));
    assert_eq!(
        reader.next().unwrap(),
        Message::Text(r#"{"id":2}"#.to_owned())
    );
    assert_eq!(reader.next().unwrap(), Message::Text(big));
    assert_eq!(reader.next().unwrap(), Message::Close);
    assert_eq!(
        reader.next().unwrap(),
        Message::Close,
        "the end of the stream"
    );
}

#[test]
fn frames_the_rfc_forbids_end_the_connection() {
    let unmasked = vec![0x81, 0x02, b'h', b'i'];
    let mut reserved = client_frame(true, 0x1, b"hi");
    reserved[0] |= 0x40;
    let long_ping = client_frame(true, 0x9, &[0; 126]);
    let stray = client_frame(true, 0x0, b"hi");
    let over = client_frame(true, 0x1, &[b'a'; 33]);
    for bytes in [unmasked, reserved, long_ping, stray] {
        assert!(
            frames::Reader::new(trickle(bytes.clone(), 3))
                .next()
                .is_err(),
            "{bytes:?}"
        );
    }
    assert!(
        frames::Reader::with_max(trickle(over, 3), 32)
            .next()
            .is_err()
    );
}

#[test]
fn server_frames_carry_each_length_form() {
    for len in [0, 125, 126, 65_535, 65_536] {
        let mut out = Vec::new();
        frames::write_text(&mut out, &vec![b'a'; len]).unwrap();
        let (payload, rest) = read_server_frame(&out);
        assert_eq!(payload.len(), len);
        assert_eq!(rest, b"");
    }
}

/// One unmasked server text frame from `bytes`: its payload and what follows it.
fn read_server_frame(bytes: &[u8]) -> (Vec<u8>, &[u8]) {
    assert_eq!(bytes[0], 0x81, "a final text frame");
    let (len, at) = match bytes[1] {
        126 => (usize::from(u16::from_be_bytes([bytes[2], bytes[3]])), 4),
        127 => (
            usize::try_from(u64::from_be_bytes(bytes[2..10].try_into().unwrap())).unwrap(),
            10,
        ),
        short => (usize::from(short), 2),
    };
    (bytes[at..at + len].to_vec(), &bytes[at + len..])
}

#[test]
fn nul_messages_come_whole_from_partial_reads() {
    let bytes = b"{\"id\":1}\0{\"method\":\"A.b\"}\0{\"id\":2,\"result\":{}}\0tail".to_vec();
    for step in [1, 3, 7, 100] {
        let mut reader = nul::Reader::new(trickle(bytes.clone(), step));
        assert_eq!(reader.next().unwrap().unwrap(), b"{\"id\":1}");
        assert_eq!(reader.next().unwrap().unwrap(), b"{\"method\":\"A.b\"}");
        assert_eq!(reader.next().unwrap().unwrap(), b"{\"id\":2,\"result\":{}}");
        assert_eq!(
            reader.next().unwrap(),
            None,
            "bytes past the last NUL are no message"
        );
    }
    let mut over = nul::Reader::with_max(trickle(vec![b'a'; 100], 10), 32);
    assert!(over.next().is_err());
    let mut out = Vec::new();
    nul::write(&mut out, b"{}").unwrap();
    assert_eq!(out, b"{}\0");
}

#[test]
fn guard_ids_and_agent_ids_never_meet() {
    let mut routes = Routes::default();
    let first = next_guard_id();
    assert!(is_guard_id(first) && is_guard_id(i64::from(i32::MAX)));
    assert!(!is_guard_id(FIRST_ID - 1) && !is_guard_id(i64::from(i32::MAX) + 1));
    assert_eq!(FIRST_ID, 1 << 30);
    // The relay's own command: its reply goes to its waiting call only.
    let (reply, inbox) = mpsc::channel();
    routes.await_reply(first, reply);
    let own = routes.browser_message(format!(r#"{{"id":{first},"result":{{"v":1}}}}"#).as_bytes());
    assert_eq!(
        own,
        FromBrowser {
            forward: false,
            guard: ToGuard::Nothing
        }
    );
    assert_eq!(inbox.try_recv().unwrap(), Ok(json!({"v": 1})));
    // A guard id the relay did not send goes to the guard's clients, never to agent-browser.
    let theirs = format!(r#"{{"id":{},"result":{{}}}}"#, first + 1);
    let routed = routes.browser_message(theirs.as_bytes());
    assert!(!routed.forward);
    assert!(matches!(routed.guard, ToGuard::Reply(id, _) if id == first + 1));
    // agent-browser's replies go to agent-browser.
    assert!(routes.browser_message(br#"{"id":7,"result":{}}"#).forward);
    // agent-browser's ids: 0 to 2^30 - 1, integers only.
    let ok = routes.client_message(r#"{"id":1073741823,"method":"Page.navigate"}"#);
    assert!(matches!(ok, FromClient::Forward(_)), "{ok:?}");
    for bad in [
        r#"{"id":1073741824}"#,
        r#"{"id":-1}"#,
        r#"{"id":2.0}"#,
        r#"{"id":1e10}"#,
    ] {
        let FromClient::Refuse(reply) = routes.client_message(bad) else {
            panic!("forwarded {bad}");
        };
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert!(reply["error"]["code"].is_i64(), "{reply}");
    }
    for dropped in [r#"{"id":"1"}"#, r#"{"method":"A.b"}"#, "[1]", "not json"] {
        assert_eq!(
            routes.client_message(dropped),
            FromClient::Drop,
            "{dropped}"
        );
    }
    // What goes to the browser is what was judged: one id, whatever the text repeated.
    let FromClient::Forward(sent) = routes.client_message(r#"{"id":1073741824,"id":5}"#) else {
        panic!("refused the last id");
    };
    assert_eq!(sent, r#"{"id":5}"#);
}

#[test]
fn a_root_fetch_event_goes_to_the_guard_and_to_agent_browser_only_after_its_own_root_enable() {
    let mut routes = Routes::default();
    routes.take_events(["Fetch.requestPaused", "Fetch.authRequired"]);
    let root = br#"{"method":"Fetch.authRequired","params":{}}"#;
    let page = br#"{"method":"Fetch.authRequired","sessionId":"S1","params":{}}"#;
    // Root: the guard's; agent-browser never enabled Fetch there.
    let routed = routes.browser_message(root);
    assert!(!routed.forward);
    assert!(matches!(routed.guard, ToGuard::Event(_)));
    // Page sessions: agent-browser's, as today, whether or not it enabled Fetch there.
    assert_eq!(
        routes.browser_message(page),
        FromBrowser {
            forward: true,
            guard: ToGuard::Nothing
        }
    );
    routes.client_message(r#"{"id":1,"method":"Fetch.enable","sessionId":"S1"}"#);
    assert!(
        !routes.browser_message(root).forward,
        "a page enable is no root enable"
    );
    // agent-browser's own root enable: both get it, until it disables.
    routes.client_message(r#"{"id":2,"method":"Fetch.enable"}"#);
    let routed = routes.browser_message(root);
    assert!(routed.forward && matches!(routed.guard, ToGuard::Event(_)));
    routes.client_message(r#"{"id":3,"method":"Fetch.disable"}"#);
    assert!(!routes.browser_message(root).forward);
    // Any other event: agent-browser's.
    let other = routes.browser_message(br#"{"method":"Page.loadEventFired","params":{}}"#);
    assert_eq!(
        other,
        FromBrowser {
            forward: true,
            guard: ToGuard::Nothing
        }
    );
}

/// The relay over a real loopback socket, with an in-memory browser. Returns the relay, the
/// browser's view of what the guard writes, and the browser's writer.
pub(super) fn relay() -> (Devtools, nul::Reader<io::PipeReader>, io::PipeWriter) {
    let (browser_reads, guard_writes) = io::pipe().unwrap();
    let (guard_reads, browser_writes) = io::pipe().unwrap();
    let devtools = Devtools::start(Box::new(guard_writes), Box::new(guard_reads)).unwrap();
    // This test process stands in for agent-browser: it is let in as its own descendant.
    devtools.allow_descendants_of(std::process::id());
    (devtools, nul::Reader::new(browser_reads), browser_writes)
}

/// Sends `request` to the relay and returns its reply's first line, and the stream.
pub(super) fn request(devtools: &Devtools, request: &str) -> (String, TcpStream) {
    let mut stream = TcpStream::connect(("127.0.0.1", devtools.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut reply = Vec::new();
    let mut byte = [0_u8; 1];
    while !reply.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
        reply.push(byte[0]);
    }
    let text = String::from_utf8(reply).unwrap();
    (text.lines().next().unwrap_or_default().to_owned(), stream)
}

pub(super) fn upgrade(devtools: &Devtools, path: &str) -> (String, TcpStream) {
    let text = head(path, "").replace("50000", &devtools.port().to_string());
    request(devtools, &text)
}

pub(super) fn right_path(devtools: &Devtools) -> String {
    devtools
        .url()
        .split_once(&format!("{}", devtools.port()))
        .unwrap()
        .1
        .to_owned()
}

/// Reads one whole server frame from `stream`.
pub(super) fn read_frame(stream: &mut TcpStream) -> Vec<u8> {
    let mut first = [0_u8; 2];
    stream.read_exact(&mut first).unwrap();
    let mut bytes = first.to_vec();
    let extra = match first[1] {
        126 => 2,
        127 => 8,
        _ => 0,
    };
    let mut ext = vec![0_u8; extra];
    stream.read_exact(&mut ext).unwrap();
    bytes.extend(&ext);
    let len = match first[1] {
        126 => usize::from(u16::from_be_bytes([ext[0], ext[1]])),
        127 => usize::try_from(u64::from_be_bytes(ext[..8].try_into().unwrap())).unwrap(),
        short => usize::from(short),
    };
    let mut payload = vec![0_u8; len];
    stream.read_exact(&mut payload).unwrap();
    bytes.extend(&payload);
    read_server_frame(&bytes).0
}

#[test]
fn the_relay_bridges_frames_and_nul_messages_both_ways() {
    let (devtools, mut browser_reads, mut browser_writes) = relay();
    assert!(devtools.url().starts_with("ws://127.0.0.1:"));
    let (status, mut client) = upgrade(&devtools, &right_path(&devtools));
    assert!(status.starts_with("HTTP/1.1 101"), "{status}");
    // Client to browser: one frame, sent in two writes, becomes one NUL-ended message.
    let frame = client_frame(true, 0x1, br#"{"id":1,"method":"Browser.getVersion"}"#);
    client.write_all(&frame[..5]).unwrap();
    client.write_all(&frame[5..]).unwrap();
    let message: Value = serde_json::from_slice(&browser_reads.next().unwrap().unwrap()).unwrap();
    assert_eq!(message, json!({"id": 1, "method": "Browser.getVersion"}));
    // Browser to client: two messages in three partial writes become two frames.
    browser_writes.write_all(b"{\"id\":1,\"res").unwrap();
    browser_writes
        .write_all(b"ult\":{}}\0{\"method\":\"Target.")
        .unwrap();
    browser_writes.write_all(b"targetCreated\"}\0").unwrap();
    assert_eq!(read_frame(&mut client), br#"{"id":1,"result":{}}"#);
    assert_eq!(
        read_frame(&mut client),
        br#"{"method":"Target.targetCreated"}"#
    );
    // A ping is answered with a pong.
    client.write_all(&client_frame(true, 0x9, b"p")).unwrap();
    let mut pong = [0_u8; 3];
    client.read_exact(&mut pong).unwrap();
    assert_eq!(pong, [0x8A, 1, b'p']);
}

#[test]
fn everything_but_the_upgrade_gets_404_and_a_second_client_is_refused() {
    let (devtools, _browser_reads, _browser_writes) = relay();
    // A bare probe, as agent-browser makes first, leaves the relay as it was.
    drop(TcpStream::connect(("127.0.0.1", devtools.port())).unwrap());
    for path in ["/json", "/json/version", "/", "/devtools/browser/0000"] {
        let (status, _) = upgrade(&devtools, path);
        assert_eq!(status, "HTTP/1.1 404 Not Found", "{path}");
    }
    let (status, _) = request(&devtools, "GET /json HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert_eq!(status, "HTTP/1.1 404 Not Found");
    let (first, _client) = upgrade(&devtools, &right_path(&devtools));
    assert!(first.starts_with("HTTP/1.1 101"), "{first}");
    let (second, _) = upgrade(&devtools, &right_path(&devtools));
    assert_eq!(second, "HTTP/1.1 404 Not Found", "one client at a time");
}

#[test]
fn a_guard_call_is_answered_on_the_pipe_and_never_reaches_the_client() {
    let (devtools, mut browser_reads, mut browser_writes) = relay();
    let (_, mut client) = upgrade(&devtools, &right_path(&devtools));
    let browser = std::thread::spawn(move || {
        let sent: Value = serde_json::from_slice(&browser_reads.next().unwrap().unwrap()).unwrap();
        assert_eq!(sent["method"], "Browser.getVersion");
        let id = sent["id"].as_i64().unwrap();
        let reply = format!("{{\"id\":{id},\"result\":{{\"product\":\"Edg/157\"}}}}\0");
        browser_writes.write_all(reply.as_bytes()).unwrap();
        browser_writes
            .write_all(b"{\"id\":3,\"result\":{}}\0")
            .unwrap();
        browser_writes
    });
    let version = devtools
        .call(
            "Browser.getVersion",
            json!({}),
            None,
            Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(version["product"], "Edg/157");
    drop(browser.join().unwrap());
    assert_eq!(
        read_frame(&mut client),
        br#"{"id":3,"result":{}}"#,
        "only agent-browser's own"
    );
}

#[test]
fn a_guard_call_fails_when_the_browser_closes_its_pipe() {
    let (devtools, browser_reads, browser_writes) = relay();
    drop(browser_writes);
    drop(browser_reads);
    let call = devtools.call(
        "Browser.getVersion",
        json!({}),
        None,
        Duration::from_secs(10),
    );
    assert!(call.is_err());
}

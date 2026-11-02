//! Plain `http://`: forwarding, header stripping, framing, and the other forms.

use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use super::harness::{
    Setup, host_denial, origin, read_all, read_head, rig, rig_allowing, serve, status,
};
use crate::Denial;
use crate::HostRefusal::{NotAllowed, Port, Scheme};
use crate::proxy::{Limits, Verdict};

const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";

/// A rig allowing `allowed.test`, at 192.0.2.20, where `origin` answers on port 80.
fn http_rig() -> (super::harness::Rig, std::sync::mpsc::Receiver<Vec<u8>>) {
    let rig = rig_allowing(&["allowed.test"]);
    let (address, received) = origin(OK);
    rig.net.name("allowed.test", &["192.0.2.20"]);
    rig.net.route("192.0.2.20:80", address);
    (rig, received)
}

#[test]
fn a_plain_get_is_forwarded_with_hop_by_hop_headers_and_the_token_stripped() {
    let (rig, received) = http_rig();
    let answer = rig.exchange(
        "GET http://Allowed.Test/a/b?q=1#frag HTTP/1.1\r\n\
         Host: evil.test\r\n\
         Proxy-Connection: keep-alive\r\n\
         Connection: keep-alive, X-Hop\r\n\
         Keep-Alive: 300\r\n\
         TE: trailers\r\n\
         Trailer: X-T\r\n\
         Upgrade: websocket\r\n\
         X-Hop: 1\r\n\
         Proxy-Authenticate: x\r\n\
         Accept: */*\r\n\
         User-Agent: test\r\n\r\n",
    );
    assert_eq!(answer, OK);
    let got = String::from_utf8(received.recv().unwrap()).unwrap();
    assert_eq!(
        got,
        "GET /a/b?q=1 HTTP/1.1\r\nHost: allowed.test\r\nAccept: */*\r\nUser-Agent: test\r\n\
         Connection: close\r\n\r\n"
    );
    let record = &rig.records(1)[0];
    assert_eq!((record.verdict, record.port), (Verdict::Forward, 80));
    assert_eq!(record.bytes_down, OK.len() as u64);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

#[test]
fn a_body_by_length_is_forwarded_and_a_second_request_is_not() {
    let (rig, received) = http_rig();
    let answer = rig.exchange(
        "POST http://allowed.test/form HTTP/1.1\r\nContent-Length: 5\r\n\r\n\
         helloGET http://allowed.test/smuggled HTTP/1.1\r\nHost: other.test\r\n\r\n",
    );
    assert_eq!(answer, OK);
    let got = String::from_utf8(received.recv().unwrap()).unwrap();
    assert_eq!(
        got,
        "POST /form HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: 5\r\n\
         Connection: close\r\n\r\nhello"
    );
}

#[test]
fn a_chunked_body_is_forwarded_unchanged_up_to_its_end() {
    let (rig, received) = http_rig();
    let body = "4\r\nWiki\r\n5;x=y\r\npedia\r\n0\r\n\r\n";
    let request = format!(
        "POST http://allowed.test/ HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n{body}\
         GET http://allowed.test/smuggled HTTP/1.1\r\n\r\n"
    );
    assert_eq!(rig.exchange(&request), OK);
    let got = String::from_utf8(received.recv().unwrap()).unwrap();
    assert_eq!(
        got,
        format!(
            "POST / HTTP/1.1\r\nHost: allowed.test\r\nTransfer-Encoding: chunked\r\n\
             Connection: close\r\n\r\n{body}"
        )
    );
}

#[test]
fn ambiguous_framing_gets_400() {
    let (rig, _received) = http_rig();
    for headers in [
        "Content-Length: 5\r\nTransfer-Encoding: chunked\r\n",
        "Content-Length: 5\r\nContent-Length: 6\r\n",
        "Content-Length: -1\r\n",
        "Transfer-Encoding: gzip, chunked\r\n",
        "Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n",
    ] {
        let answer = rig.exchange(&format!(
            "POST http://allowed.test/ HTTP/1.1\r\n{headers}\r\n"
        ));
        assert!(answer.starts_with("HTTP/1.1 400 "), "{headers:?}: {answer}");
    }
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn an_unlisted_http_host_gets_403_and_a_denial() {
    let (rig, _received) = http_rig();
    let answer = rig.exchange("GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 403 "), "{answer}");
    let answer = rig.exchange("GET http://allowed.test:8080/ HTTP/1.1\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 403 "), "{answer}");
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("example.com", 80, NotAllowed, false),
            host_denial("allowed.test", 8080, Port, false),
        ]
    );
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn origin_form_and_other_schemes_get_405_with_why_scheme() {
    let (rig, _received) = http_rig();
    for request in [
        "GET / HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
        "GET ftp://files.test/a HTTP/1.1\r\n\r\n",
        "GET https://allowed.test/ HTTP/1.1\r\n\r\n",
        "OPTIONS * HTTP/1.1\r\nHost: allowed.test:8080\r\n\r\n",
    ] {
        let answer = rig.exchange(request);
        assert!(answer.starts_with("HTTP/1.1 405 "), "{request:?}: {answer}");
    }
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("allowed.test", 80, Scheme, false),
            host_denial("files.test", 21, Scheme, false),
            host_denial("allowed.test", 443, Scheme, false),
            host_denial("allowed.test", 8080, Scheme, false),
        ]
    );
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn a_request_with_no_host_to_name_gets_400_and_no_denial() {
    let (rig, _received) = http_rig();
    for request in [
        // Origin-form with no Host header.
        "GET /x HTTP/1.1\r\n\r\n",
        // A lowercase connect is not CONNECT: an authority-form target needs CONNECT.
        "connect example.com:443 HTTP/1.1\r\n\r\n",
        "GET allowed.test:80 HTTP/1.1\r\n\r\n",
    ] {
        let answer = rig.exchange(request);
        assert!(answer.starts_with("HTTP/1.1 400 "), "{request:?}: {answer}");
    }
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    assert!(
        rig.records(3)
            .iter()
            .all(|r| r.verdict == Verdict::BadRequest)
    );
}

/// An origin that keeps everything it receives until the proxy closes, and answers nothing.
fn sink_origin() -> (SocketAddr, mpsc::Receiver<Vec<u8>>) {
    let (sent, received) = mpsc::channel();
    let sent = Mutex::new(sent);
    let address = serve(move |mut stream| {
        let mut got = Vec::new();
        drop(stream.read_to_end(&mut got));
        sent.lock().unwrap().send(got).unwrap();
    });
    (address, received)
}

#[test]
fn a_chunk_size_line_with_anything_but_an_extension_after_the_size_gets_400() {
    for size_line in ["5 6", "5 x"] {
        let rig = rig_allowing(&["allowed.test"]);
        let (address, received) = sink_origin();
        rig.net.name("allowed.test", &["192.0.2.20"]);
        rig.net.route("192.0.2.20:80", address);
        let answer = rig.exchange(&format!(
            "POST http://allowed.test/ HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n\
             {size_line}\r\nhello\r\n0\r\n\r\n"
        ));
        assert!(
            answer.starts_with("HTTP/1.1 400 "),
            "{size_line:?}: {answer}"
        );
        let got = String::from_utf8(received.recv().unwrap()).unwrap();
        assert!(
            !got.contains(size_line),
            "{size_line:?} reached the origin: {got:?}"
        );
        assert_eq!(rig.records(1)[0].verdict, Verdict::BadRequest);
    }
}

#[test]
fn a_head_with_bare_lf_line_ends_gets_400_at_once() {
    // A head limit far past the client's wait: a 408 would fail the test, only a prompt 400
    // passes.
    let rig = rig(Setup {
        allow: vec!["allowed.test"],
        limits: Limits {
            head: Duration::from_secs(600),
            ..Limits::default()
        },
        ..Setup::default()
    });
    for request in [
        &b"GET http://allowed.test/ HTTP/1.1\nHost: allowed.test\n\n"[..],
        // httparse would stop at the bare-LF blank line and drop X-Hidden.
        b"GET http://allowed.test/ HTTP/1.1\r\nHost: allowed.test\n\nX-Hidden: 1\r\n\r\n",
    ] {
        let mut stream = rig.agent();
        stream.write_all(request).unwrap();
        assert_eq!(status(&read_head(&mut stream)), 400, "{request:?}");
        assert_eq!(read_all(&mut stream), b"");
    }
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn a_silent_host_on_another_port_is_refused_with_no_denial() {
    let rig = rig(Setup {
        silent: vec!["telemetry.test"],
        ..Setup::default()
    });
    let answer = rig.exchange("GET http://telemetry.test:8080/ HTTP/1.1\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 403 "), "{answer}");
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    assert_eq!(rig.records(1)[0].verdict, Verdict::Silent);
}

#[test]
fn a_forwarded_request_idle_past_the_limit_is_closed() {
    let rig = rig(Setup {
        allow: vec!["allowed.test"],
        limits: Limits {
            idle: Duration::from_millis(100),
            ..Limits::default()
        },
        ..Setup::default()
    });
    let (address, received) = sink_origin();
    rig.net.name("allowed.test", &["192.0.2.20"]);
    rig.net.route("192.0.2.20:80", address);
    // Three of ten body bytes, then nothing: the proxy closes both sides.
    let answer =
        rig.exchange("POST http://allowed.test/ HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc");
    assert_eq!(answer, "");
    assert!(
        String::from_utf8(received.recv().unwrap())
            .unwrap()
            .ends_with("abc")
    );
    assert_eq!(rig.records(1)[0].verdict, Verdict::Forward);
}

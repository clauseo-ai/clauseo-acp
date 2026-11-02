//! Caps, timeouts, the handle's lifetime, connectivity, and what the handle shows.

use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use super::harness::{Setup, echo, read_all, read_head, rig, rig_allowing, status, token_of};
use crate::Denial;
use crate::Reachability::{Reachable, Unreachable};
use crate::proxy::head::HEAD_CAP;
use crate::proxy::{Limits, Verdict};

#[test]
fn the_sixty_fifth_open_tunnel_gets_503_and_no_denial() {
    let rig = rig_allowing(&["api.anthropic.com"]);
    rig.net.name("api.anthropic.com", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    let open: Vec<TcpStream> = (0..64)
        .map(|i| {
            let (tunnel, code) = rig.connect("api.anthropic.com:443");
            assert_eq!(code, 200, "tunnel {i}");
            tunnel
        })
        .collect();
    assert_eq!(rig.connect("api.anthropic.com:443").1, 503);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    drop(open);
}

#[test]
fn a_head_over_8_kb_gets_431() {
    let rig = rig_allowing(&["api.anthropic.com"]);
    let mut stream = rig.agent();
    let filler = "a".repeat(HEAD_CAP);
    let request = format!("CONNECT api.anthropic.com:443 HTTP/1.1\r\nX-Filler: {filler}\r\n\r\n");
    // The proxy may close before reading it all; the answer is what counts.
    drop(stream.write_all(request.as_bytes()));
    assert_eq!(status(&read_head(&mut stream)), 431);
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn a_head_not_finished_in_time_gets_408() {
    // A 100 ms head limit in place of 10 s, so the test waits on an async timer, not a sleep.
    let rig = rig(Setup {
        limits: Limits {
            head: Duration::from_millis(100),
            ..Limits::default()
        },
        ..Setup::default()
    });
    let mut stream = rig.agent();
    stream
        .write_all(b"CONNECT api.anthropic.com:443 HTTP/1.1\r\n")
        .unwrap();
    assert_eq!(status(&read_head(&mut stream)), 408);
    assert_eq!(read_all(&mut stream), b"");
}

#[test]
fn malformed_requests_get_400() {
    let rig = rig_allowing(&["api.anthropic.com"]);
    for request in [
        "CONNECT api.anthropic.com HTTP/1.1\r\n\r\n",
        "CONNECT api.anthropic.com:+443 HTTP/1.1\r\n\r\n",
        "CONNECT user@api.anthropic.com:443 HTTP/1.1\r\n\r\n",
        "CONNECT ::1:443 HTTP/1.1\r\n\r\n",
        "CONNECT api.anthropic.com:99999 HTTP/1.1\r\n\r\n",
        "GET http://user:pw@api.anthropic.com/ HTTP/1.1\r\n\r\n",
        "NOT HTTP\r\n\r\n",
    ] {
        let answer = rig.exchange(request);
        assert!(answer.starts_with("HTTP/1.1 400 "), "{request:?}: {answer}");
    }
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

#[test]
fn dropping_the_handle_closes_both_ports_and_every_tunnel() {
    let rig = rig_allowing(&["api.anthropic.com"]);
    rig.net.name("api.anthropic.com", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    let (mut tunnel, code) = rig.connect("api.anthropic.com:443");
    assert_eq!(code, 200);
    let (port, browser_port) = (rig.handle.port(), rig.handle.browser_port());
    drop(rig.handle);
    assert_eq!(read_all(&mut tunnel), b"", "the tunnel ended");
    for port in [port, browser_port] {
        let refused = TcpStream::connect(("127.0.0.1", port)).unwrap_err();
        assert_eq!(
            refused.kind(),
            std::io::ErrorKind::ConnectionRefused,
            "{port}"
        );
    }
}

#[test]
fn three_failures_in_a_row_are_unreachable_and_the_next_success_reachable() {
    let rig = rig_allowing(&["up.test", "down.test", "nowhere.test"]);
    rig.net.name("up.test", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    // down.test resolves to an address nothing answers; nowhere.test does not resolve.
    rig.net.name("down.test", &["192.0.2.99"]);
    assert_eq!(rig.connect("up.test:443").1, 200);
    assert_eq!(rig.reach(), vec![Reachable]);
    assert_eq!(rig.connect("down.test:443").1, 502);
    assert_eq!(rig.connect("nowhere.test:443").1, 502);
    // Refusals do not count.
    assert_eq!(rig.connect("example.com:443").1, 403);
    assert_eq!(rig.reach(), vec![Reachable]);
    assert_eq!(rig.connect("down.test:443").1, 502);
    assert_eq!(rig.reach(), vec![Reachable, Unreachable]);
    assert_eq!(rig.connect("down.test:443").1, 502);
    assert_eq!(rig.reach(), vec![Reachable, Unreachable]);
    assert_eq!(rig.connect("up.test:443").1, 200);
    assert_eq!(rig.reach(), vec![Reachable, Unreachable, Reachable]);
    let failed = rig
        .records()
        .iter()
        .filter(|r| r.verdict == Verdict::Failed)
        .count();
    assert_eq!(failed, 4);
}

#[test]
fn env_points_at_the_agent_listener_and_debug_hides_the_token() {
    let rig = rig_allowing(&[]);
    let token = token_of(&rig.handle);
    assert_eq!(token.len(), 64);
    let url = format!("http://clauseo:{token}@127.0.0.1:{}", rig.handle.port());
    let env: Vec<(String, Option<String>)> = rig
        .handle
        .env()
        .into_iter()
        .map(|(k, v)| {
            (
                k.into_string().unwrap(),
                v.map(|v| v.into_string().unwrap()),
            )
        })
        .collect();
    let set = |k: &str, v: &str| (k.to_owned(), Some(v.to_owned()));
    assert_eq!(
        env,
        vec![
            set("HTTPS_PROXY", &url),
            set("HTTP_PROXY", &url),
            set("https_proxy", &url),
            set("http_proxy", &url),
            set("NO_PROXY", ""),
            set("no_proxy", ""),
            ("ALL_PROXY".to_owned(), None),
            ("all_proxy".to_owned(), None),
        ]
    );
    let debug = format!("{:?}", rig.handle);
    assert!(!debug.contains(&token), "{debug}");
    assert!(debug.contains("<redacted>"), "{debug}");
    assert_ne!(rig.handle.port(), rig.handle.browser_port());
}

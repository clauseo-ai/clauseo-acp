//! Caps, timeouts, the handle's lifetime, connectivity, and what the handle shows.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use super::harness::{
    Setup, echo, read_all, read_head, rig, rig_allowing, round_trip, status, token_of,
};
use crate::Denial;
use crate::Reachability::{Reachable, Unreachable};
use crate::proxy::head::HEAD_CAP;
use crate::proxy::relay::LINGER_CAP;
use crate::proxy::{Limits, ProxyHandle, Verdict};

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
    let request = oversized_head();
    assert!(request.len() > HEAD_CAP);
    // The proxy reads and discards what is left after its answer, so the whole write lands and
    // the close is clean: the 431 arrives whole, then the end, with no reset (Windows sent one).
    stream.write_all(request.as_bytes()).unwrap();
    assert_eq!(status(&read_head(&mut stream)), 431);
    assert_eq!(read_all(&mut stream), b"");
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

fn oversized_head() -> String {
    let filler = "a".repeat(HEAD_CAP);
    format!("CONNECT api.anthropic.com:443 HTTP/1.1\r\nX-Filler: {filler}\r\n\r\n")
}

#[test]
fn a_client_that_stays_open_after_an_error_answer_is_closed_after_the_linger() {
    assert_eq!(Limits::default().linger, Duration::from_secs(1));
    let rig = echo_rig_with(Limits {
        sockets: 2,
        linger: Duration::from_millis(100),
        ..Limits::default()
    });
    let mut first = rig.agent();
    first.write_all(oversized_head().as_bytes()).unwrap();
    assert_eq!(status(&read_head(&mut first)), 431);
    // The client neither reads on nor closes. The record is written after the socket slot is
    // given back, so once it is in, the next client and its origin fit in the two slots.
    assert_eq!(rig.records(1)[0].verdict, Verdict::BadRequest);
    assert_eq!(rig.connect("api.anthropic.com:443").1, 200);
    drop(first);
}

#[test]
fn the_linger_stops_reading_at_its_byte_cap() {
    let rig = echo_rig_with(Limits {
        // Longer than the test waits: only the byte cap can end the linger.
        linger: Duration::from_mins(5),
        ..Limits::default()
    });
    let mut stream = rig.agent();
    let mut flood = oversized_head().into_bytes();
    flood.resize(flood.len() + LINGER_CAP + 16 * 1024, b'a');
    // The proxy closes once it has discarded the cap, so the end of the write may be reset.
    drop(stream.write_all(&flood));
    assert_eq!(rig.records(1)[0].verdict, Verdict::BadRequest);
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
    // One idle connection on each listener, accepted or still queued, opened before the drop.
    // A connect made after it proves nothing: the port is free then, and another test process
    // can bind it first (it did, on CI).
    let idle = [("agent", rig.agent()), ("browser", rig.browser())];
    drop(rig.handle);
    assert_eq!(read_all(&mut tunnel), b"", "the tunnel ended");
    for (listener, mut stream) in idle {
        // The proxy closed it (EOF), or its listener closed with it still queued (a reset).
        // A proxy that kept it would answer the 10 s head timeout, or let the read time out.
        let read = stream.read(&mut [0u8; 1]);
        let ended = match &read {
            Ok(n) => *n == 0,
            Err(error) => matches!(
                error.kind(),
                ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
            ),
        };
        assert!(ended, "the {listener} listener kept a connection: {read:?}");
    }
    // The proxy thread let go of the sinks it was given: its tasks are gone too.
    assert_eq!(Arc::strong_count(&rig.records), 1, "the record callback");
    assert_eq!(Arc::strong_count(&rig.sink), 1, "the denial sink");
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
        .records(7)
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

/// Reads until the proxy closes: nothing, whether the close came as an end or a reset.
fn closed_unanswered(stream: &mut TcpStream) -> bool {
    let mut buf = Vec::new();
    match std::io::Read::read_to_end(stream, &mut buf) {
        Ok(_) => buf.is_empty(),
        Err(error) => error.kind() == std::io::ErrorKind::ConnectionReset,
    }
}

fn echo_rig_with(limits: Limits) -> super::harness::Rig {
    let rig = rig(Setup {
        allow: vec!["api.anthropic.com"],
        limits,
        ..Setup::default()
    });
    rig.net.name("api.anthropic.com", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    rig
}

#[test]
fn connections_past_the_cap_on_unfinished_heads_are_closed_until_one_finishes() {
    let rig = echo_rig_with(Limits {
        pending: 2,
        ..Limits::default()
    });
    assert_eq!(Limits::default().pending, 64);
    // Two connections that never send a head fill the cap; the accept loop takes them in order.
    let mut first = rig.agent();
    let _second = rig.agent();
    let mut third = rig.agent();
    assert!(closed_unanswered(&mut third), "the third is closed");
    // The first finishes its head: its slot frees, and the next connection is served.
    let request = format!(
        "CONNECT api.anthropic.com:443 HTTP/1.1\r\n{}\r\n",
        rig.auth()
    );
    first.write_all(request.as_bytes()).unwrap();
    assert_eq!(status(&read_head(&mut first)), 200);
    assert_eq!(rig.connect("api.anthropic.com:443").1, 200);
}

#[test]
fn origin_sockets_count_against_the_launch_socket_cap() {
    let rig = echo_rig_with(Limits {
        sockets: 2,
        ..Limits::default()
    });
    let _idle = rig.agent();
    // The second client fits, but its origin socket would be the third: 503, and no denial.
    assert_eq!(rig.connect("api.anthropic.com:443").1, 503);
    assert_eq!(rig.records(1)[0].verdict, Verdict::Busy);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

// nextest runs each test in its own process, so the process-wide count starts at zero in each.

#[test]
fn the_process_wide_socket_cap_counts_origin_sockets_of_other_launches() {
    let limits = Limits {
        process: 3,
        ..Limits::default()
    };
    assert_eq!(Limits::default().process, 1024);
    let one = echo_rig_with(limits);
    let two = echo_rig_with(limits);
    let (_tunnel, code) = one.connect("api.anthropic.com:443");
    assert_eq!(code, 200, "a client and an origin socket");
    // The other launch's client is the third socket; its origin socket would be the fourth.
    assert_eq!(two.connect("api.anthropic.com:443").1, 503);
}

#[test]
fn the_process_wide_socket_cap_closes_clients_of_other_launches() {
    let limits = Limits {
        process: 2,
        ..Limits::default()
    };
    let one = echo_rig_with(limits);
    let two = echo_rig_with(limits);
    let (_tunnel, code) = one.connect("api.anthropic.com:443");
    assert_eq!(code, 200);
    let mut third = two.agent();
    assert!(
        closed_unanswered(&mut third),
        "a client past the cap is closed"
    );
}

#[test]
fn a_tunnel_idle_past_the_limit_is_closed() {
    assert_eq!(Limits::default().idle, Duration::from_mins(15));
    let rig = echo_rig_with(Limits {
        idle: Duration::from_millis(100),
        ..Limits::default()
    });
    let (mut tunnel, code) = rig.connect("api.anthropic.com:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ping"), b"ping");
    // Then nothing either way: the proxy closes the tunnel.
    assert_eq!(read_all(&mut tunnel), b"");
    let record = &rig.records(1)[0];
    assert_eq!((record.verdict, record.bytes_up), (Verdict::Tunnel, 4));
}

#[test]
fn idle_browser_connections_cannot_lock_out_the_agent_listener() {
    let rig = echo_rig_with(Limits {
        pending: 2,
        ..Limits::default()
    });
    // The token-free browser listener's pending slots fill, and its next connection is closed.
    let _first = rig.browser();
    let _second = rig.browser();
    let mut third = rig.browser();
    assert!(closed_unanswered(&mut third));
    // The agent listener has its own.
    assert_eq!(rig.connect("api.anthropic.com:443").1, 200);
}

#[test]
fn dns_past_the_connect_timeout_answers_504() {
    let rig = echo_rig_with(Limits {
        connect: Duration::from_millis(100),
        ..Limits::default()
    });
    // Never released: the lookup outlives the timeout.
    let _held = rig.net.hold_next_resolve();
    assert_eq!(rig.connect("api.anthropic.com:443").1, 504);
    assert_eq!(rig.records(1)[0].verdict, Verdict::Failed);
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn a_route_lookup_past_the_connect_timeout_goes_direct() {
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let wait = std::sync::Mutex::new(wait);
    // An OS lookup that never answers while the test runs.
    let hung = move |_: crate::proxy::upstream::Scheme, _: &str, _: u16| {
        if wait.lock().unwrap().recv().is_err() {
            // Released by the test dropping its end.
        }
        crate::Route::Http {
            host: "127.0.0.1".to_owned(),
            port: 9,
        }
    };
    let rig = rig(Setup {
        allow: vec!["api.anthropic.com"],
        upstream: crate::Upstream::System,
        os: Some(Box::new(hung)),
        limits: Limits {
            connect: Duration::from_millis(100),
            ..Limits::default()
        },
        ..Setup::default()
    });
    rig.net.name("api.anthropic.com", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    let (mut tunnel, code) = rig.connect("api.anthropic.com:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"direct"), b"direct");
    let direct: SocketAddr = "192.0.2.10:443".parse().unwrap();
    assert_eq!(rig.net.dialled(), vec![direct]);
    drop(release);
}

/// Whether dropping `handle` on another thread returns within a second: the quit path joins the
/// proxy thread, so nothing in flight may hold it.
fn drops_within_a_second(handle: ProxyHandle) -> bool {
    let (dropped, done) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(handle);
        // The test may have stopped waiting.
        match dropped.send(()) {
            Ok(()) | Err(std::sync::mpsc::SendError(())) => {}
        }
    });
    done.recv_timeout(Duration::from_secs(1)).is_ok()
}

#[test]
fn dropping_the_handle_during_an_origin_connect_that_never_answers_returns_at_once() {
    let rig = echo_rig_with(Limits::default());
    // Never released: an origin that never answers, as a blackholed address would.
    let held = rig.net.hold_next_connect();
    let mut stream = rig.agent();
    let request = format!(
        "CONNECT api.anthropic.com:443 HTTP/1.1\r\n{}\r\n",
        rig.auth()
    );
    stream.write_all(request.as_bytes()).unwrap();
    held.arrived.recv().unwrap();
    assert!(
        drops_within_a_second(rig.handle),
        "the drop waited on the connect"
    );
    assert_eq!(read_all(&mut stream), b"", "closed with no 200");
    drop(held.release);
}

#[test]
fn dropping_the_handle_during_a_route_lookup_that_never_answers_returns_at_once() {
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let (arrived, lookup) = std::sync::mpsc::channel::<()>();
    let held = std::sync::Mutex::new((arrived, wait));
    // An OS lookup (a PAC fetch, say) that never answers while the test runs; it runs on the
    // blocking pool, which the drop must not wait for.
    let hung = move |_: crate::proxy::upstream::Scheme, _: &str, _: u16| {
        let held = held.lock().unwrap();
        // The test may have stopped waiting.
        match held.0.send(()) {
            Ok(()) | Err(std::sync::mpsc::SendError(())) => {}
        }
        if held.1.recv().is_err() {
            // Released by the test dropping its end.
        }
        crate::Route::Direct
    };
    let rig = rig(Setup {
        allow: vec!["api.anthropic.com"],
        upstream: crate::Upstream::System,
        os: Some(Box::new(hung)),
        ..Setup::default()
    });
    rig.net.name("api.anthropic.com", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    let mut stream = rig.agent();
    let request = format!(
        "CONNECT api.anthropic.com:443 HTTP/1.1\r\n{}\r\n",
        rig.auth()
    );
    stream.write_all(request.as_bytes()).unwrap();
    lookup.recv().unwrap();
    assert!(
        drops_within_a_second(rig.handle),
        "the drop waited on the lookup"
    );
    assert_eq!(read_all(&mut stream), b"", "closed with no 200");
    drop(release);
}

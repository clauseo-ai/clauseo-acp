//! CONNECT: the token, the lists, ports, addresses, allow and revoke.

use std::io::{Read, Write};
use std::net::SocketAddr;

use super::harness::{
    Setup, auth_line, echo, host_denial, read_all, read_head, rig, rig_allowing, round_trip,
    status, token_of,
};
use crate::Denial;
use crate::HostRefusal::{NotAllowed, Port, PrivateAddress};
use crate::proxy::{Listener, Verdict};

/// A rig allowing `host`, which resolves to 192.0.2.10, where an echo server answers on 443.
fn echo_rig(allow: &[&'static str], host: &str) -> super::harness::Rig {
    let rig = rig_allowing(allow);
    rig.net.name(host, &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    rig
}

#[test]
fn an_allowed_connect_gets_200_and_bytes_pass_both_ways_unchanged() {
    let rig = echo_rig(&["api.anthropic.com"], "api.anthropic.com");
    let (mut tunnel, code) = rig.connect("api.anthropic.com:443");
    assert_eq!(code, 200);
    let data: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
    assert_eq!(round_trip(&mut tunnel, &data), data);
    tunnel.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(read_all(&mut tunnel), b"");
    let dialled: SocketAddr = "192.0.2.10:443".parse().unwrap();
    assert_eq!(rig.net.dialled(), vec![dialled]);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    let records = rig.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(
        (record.verdict, record.host.as_str(), record.port),
        (Verdict::Tunnel, "api.anthropic.com", 443)
    );
    assert_eq!((record.bytes_up, record.bytes_down), (100_000, 100_000));
}

#[test]
fn bytes_sent_before_the_200_reach_the_origin() {
    let rig = echo_rig(&["api.anthropic.com"], "api.anthropic.com");
    let mut stream = rig.agent();
    let request = format!(
        "CONNECT api.anthropic.com:443 HTTP/1.1\r\n{}\r\nearly",
        rig.auth()
    );
    stream.write_all(request.as_bytes()).unwrap();
    assert_eq!(status(&read_head(&mut stream)), 200);
    let mut back = [0u8; 5];
    stream.read_exact(&mut back).unwrap();
    assert_eq!(&back, b"early");
}

#[test]
fn the_agent_listener_refuses_a_missing_or_wrong_token_and_another_launches_token() {
    let rig = echo_rig(&["api.anthropic.com"], "api.anthropic.com");
    let other = rig_allowing(&["api.anthropic.com"]);
    let wrong = "0".repeat(64);
    let lines = [
        String::new(),
        auth_line(&wrong),
        auth_line(&token_of(&other.handle)),
        "Proxy-Authorization: Bearer x\r\n".to_owned(),
        format!("{}{}", rig.auth(), rig.auth()),
    ];
    for line in lines {
        let mut stream = rig.agent();
        let request = format!("CONNECT api.anthropic.com:443 HTTP/1.1\r\n{line}\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let head = read_head(&mut stream);
        assert_eq!(status(&head), 407, "{line:?}");
        assert!(
            head.contains("Proxy-Authenticate: Basic realm=\"clauseo\"\r\n"),
            "{head}"
        );
        assert_eq!(read_all(&mut stream), b"", "no tunnel after a 407");
    }
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    assert!(
        rig.records()
            .iter()
            .all(|r| r.verdict == Verdict::AuthRequired)
    );
    // The record names the host asked for, for the example's log.
    assert!(
        rig.records()
            .iter()
            .all(|r| (r.host.as_str(), r.port) == ("api.anthropic.com", 443)),
        "{:?}",
        rig.records()
    );
}

#[test]
fn the_browser_listener_needs_no_token_under_the_same_list_and_sink() {
    let rig = echo_rig(&["api.anthropic.com"], "api.anthropic.com");
    let mut stream = rig.browser();
    stream
        .write_all(b"CONNECT api.anthropic.com:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    assert_eq!(status(&read_head(&mut stream)), 200);
    assert_eq!(round_trip(&mut stream, b"hello"), b"hello");
    let mut refused = rig.browser();
    refused
        .write_all(b"CONNECT example.com:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    assert_eq!(status(&read_head(&mut refused)), 403);
    drop(read_all(&mut refused));
    assert_eq!(
        rig.denials(),
        vec![host_denial("example.com", 443, NotAllowed, false)]
    );
    let listeners: Vec<Listener> = rig.records().iter().map(|r| r.listener).collect();
    assert_eq!(listeners, vec![Listener::Browser]);
}

#[test]
fn an_unlisted_host_is_refused_and_denied_on_every_try() {
    let rig = echo_rig(&["api.anthropic.com"], "api.anthropic.com");
    for _ in 0..3 {
        let (_, code) = rig.connect("example.com:443");
        assert_eq!(code, 403);
    }
    let (_, code) = rig.connect("raw.githubusercontent.com:443");
    assert_eq!(code, 403);
    let mut expected = vec![host_denial("example.com", 443, NotAllowed, false); 3];
    expected.push(host_denial(
        "raw.githubusercontent.com",
        443,
        NotAllowed,
        true,
    ));
    assert_eq!(rig.denials(), expected);
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
    assert!(
        rig.net.resolved().is_empty(),
        "a refused name is never resolved"
    );
}

#[test]
fn the_rate_cap_holds_denials_to_twenty_a_window() {
    let rig = rig_allowing(&[]);
    for _ in 0..25 {
        assert_eq!(rig.connect("example.com:443").1, 403);
    }
    assert_eq!(rig.denials().len(), 20);
    rig.clock.advance(crate::DENIAL_WINDOW);
    assert_eq!(rig.connect("example.com:443").1, 403);
    let denials = rig.denials();
    assert_eq!(denials.len(), 22);
    assert_eq!(denials[20], crate::Denial::Dropped { count: 5 });
    assert_eq!(
        denials[21],
        host_denial("example.com", 443, NotAllowed, false)
    );
}

#[test]
fn a_silent_host_is_refused_with_no_denial() {
    let rig = rig(Setup {
        allow: vec!["api.anthropic.com"],
        silent: vec!["telemetry.test"],
        ..Setup::default()
    });
    let (mut stream, code) = rig.connect("telemetry.test:443");
    assert_eq!(code, 403);
    drop(read_all(&mut stream));
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    assert_eq!(rig.records()[0].verdict, Verdict::Silent);
}

#[test]
fn a_connect_to_another_port_is_refused_with_why_port() {
    let rig = echo_rig(&["api.anthropic.com"], "api.anthropic.com");
    for target in [
        "api.anthropic.com:80",
        "api.anthropic.com:8443",
        "api.anthropic.com:22",
    ] {
        assert_eq!(rig.connect(target).1, 403, "{target}");
    }
    let ports: Vec<_> = rig.denials();
    assert_eq!(
        ports,
        vec![
            host_denial("api.anthropic.com", 80, Port, false),
            host_denial("api.anthropic.com", 8443, Port, false),
            host_denial("api.anthropic.com", 22, Port, false),
        ]
    );
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn a_host_that_resolves_to_a_non_public_address_is_refused() {
    let cases: &[&[&str]] = &[
        &["127.0.0.1"],
        &["10.0.0.5"],
        &["172.16.4.4"],
        &["192.168.1.1"],
        &["169.254.169.254"],
        &["100.64.0.1"],
        &["0.0.0.0"],
        &["::1"],
        &["::"],
        &["fd00::1"],
        &["fe80::1"],
        &["::ffff:127.0.0.1"],
        &["::ffff:10.0.0.1"],
        // One private address among public ones is enough.
        &["192.0.2.10", "10.0.0.1"],
    ];
    for &ips in cases {
        let rig = rig_allowing(&["inside.test"]);
        rig.net.name("inside.test", ips);
        rig.net.route("192.0.2.10:443", echo());
        assert_eq!(rig.connect("inside.test:443").1, 403, "{ips:?}");
        assert_eq!(
            rig.denials(),
            vec![host_denial("inside.test", 443, PrivateAddress, false)],
            "{ips:?}"
        );
        assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new(), "{ips:?}");
    }
}

#[test]
fn the_proxy_connects_to_the_address_it_checked_and_never_resolves_again() {
    let rig = rig_allowing(&["rebind.test"]);
    // A second lookup would answer loopback (DNS rebinding); there must be no second lookup.
    rig.net
        .answers("rebind.test", &[&["192.0.2.10"], &["127.0.0.1"]]);
    rig.net.route("192.0.2.10:443", echo());
    let (mut tunnel, code) = rig.connect("rebind.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ping"), b"ping");
    assert_eq!(rig.net.resolved(), vec!["rebind.test".to_owned()]);
    let checked: SocketAddr = "192.0.2.10:443".parse().unwrap();
    assert_eq!(rig.net.dialled(), vec![checked]);
}

#[test]
fn an_ip_literal_is_refused_unless_listed_and_a_listed_one_is_still_checked() {
    let rig = rig_allowing(&["api.anthropic.com", "192.0.2.10", "127.0.0.1", "[::1]"]);
    rig.net.route("192.0.2.10:443", echo());
    assert_eq!(rig.connect("192.0.2.11:443").1, 403);
    assert_eq!(rig.connect("[2001:db8::1]:443").1, 403);
    assert_eq!(rig.connect("127.0.0.1:443").1, 403);
    assert_eq!(rig.connect("0x7f.0.0.1:443").1, 403);
    assert_eq!(rig.connect("[::1]:443").1, 403);
    let (mut tunnel, code) = rig.connect("192.0.2.10:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("192.0.2.11", 443, NotAllowed, false),
            host_denial("2001:db8::1", 443, NotAllowed, false),
            host_denial("127.0.0.1", 443, PrivateAddress, false),
            host_denial("127.0.0.1", 443, PrivateAddress, false),
            host_denial("::1", 443, PrivateAddress, false),
        ]
    );
    assert_eq!(
        rig.net.resolved(),
        Vec::<String>::new(),
        "literals are never resolved"
    );
}

#[test]
fn names_are_matched_without_case_trailing_dot_or_unicode() {
    let rig = rig_allowing(&["bücher.example", "*.firm.example"]);
    rig.net.name("xn--bcher-kva.example", &["192.0.2.10"]);
    rig.net.name("docs.firm.example", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    for target in [
        "XN--BCHER-KVA.example.:443",
        "Docs.Firm.Example.:443",
        "docs.firm.example:443",
    ] {
        assert_eq!(rig.connect(target).1, 200, "{target}");
    }
    assert_eq!(rig.connect("firm.example:443").1, 403);
}

#[test]
fn allow_opens_the_next_connect_and_revoke_closes_open_tunnels() {
    let rig = echo_rig(&["api.anthropic.com"], "site.test");
    assert_eq!(rig.connect("site.test:443").1, 403);
    rig.handle.allow("Site.Test.");
    let (mut tunnel, code) = rig.connect("site.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"one"), b"one");
    rig.handle.revoke("site.test");
    assert_eq!(read_all(&mut tunnel), b"", "revoke closes the tunnel");
    assert_eq!(rig.connect("site.test:443").1, 403);
    // Revoke also takes a host off the start list.
    rig.net.name("api.anthropic.com", &["192.0.2.10"]);
    rig.handle.revoke("api.anthropic.com");
    assert_eq!(rig.connect("api.anthropic.com:443").1, 403);
}

#[test]
fn revoke_keeps_a_host_a_suffix_entry_still_covers() {
    let rig = echo_rig(&["*.firm.example"], "docs.firm.example");
    rig.handle.allow("docs.firm.example");
    let (mut tunnel, code) = rig.connect("docs.firm.example:443");
    assert_eq!(code, 200);
    rig.handle.revoke("docs.firm.example");
    assert_eq!(round_trip(&mut tunnel, b"still"), b"still");
    assert_eq!(rig.connect("docs.firm.example:443").1, 200);
}

#[test]
fn a_silent_host_on_another_port_gets_403_and_no_denial() {
    let rig = rig(Setup {
        silent: vec!["telemetry.test"],
        ..Setup::default()
    });
    assert_eq!(rig.connect("telemetry.test:8443").1, 403);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    assert_eq!(rig.records()[0].verdict, Verdict::Silent);
}

#[test]
fn a_revoke_during_dns_refuses_the_connect() {
    let rig = echo_rig(&["api.anthropic.com"], "api.anthropic.com");
    let held = rig.net.hold_next_resolve();
    let mut stream = rig.agent();
    let request = format!(
        "CONNECT api.anthropic.com:443 HTTP/1.1\r\n{}\r\n",
        rig.auth()
    );
    stream.write_all(request.as_bytes()).unwrap();
    held.arrived.recv().unwrap();
    rig.handle.revoke("api.anthropic.com");
    held.release.send(()).unwrap();
    assert_eq!(status(&read_head(&mut stream)), 403);
    assert_eq!(
        rig.denials(),
        vec![host_denial("api.anthropic.com", 443, NotAllowed, false)]
    );
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn a_revoke_during_the_origin_connect_ends_the_tunnel_before_its_200() {
    let rig = echo_rig(&["api.anthropic.com"], "api.anthropic.com");
    let held = rig.net.hold_next_connect();
    let mut stream = rig.agent();
    let request = format!(
        "CONNECT api.anthropic.com:443 HTTP/1.1\r\n{}\r\n",
        rig.auth()
    );
    stream.write_all(request.as_bytes()).unwrap();
    held.arrived.recv().unwrap();
    rig.handle.revoke("api.anthropic.com");
    // The release may find the connect already dropped with its tunnel.
    match held.release.send(()) {
        Ok(()) | Err(()) => {}
    }
    assert_eq!(read_all(&mut stream), b"", "closed with no 200");
    assert_eq!(rig.connect("api.anthropic.com:443").1, 403);
}

#[test]
fn a_half_closed_tunnel_passes_the_end_on_and_keeps_the_other_way_open() {
    let rig = echo_rig(&["api.anthropic.com"], "api.anthropic.com");
    let (mut tunnel, code) = rig.connect("api.anthropic.com:443");
    assert_eq!(code, 200);
    tunnel.write_all(b"last words").unwrap();
    tunnel.shutdown(std::net::Shutdown::Write).unwrap();
    // The echo server sees the end only if the proxy passed it on; its reply still comes back.
    assert_eq!(read_all(&mut tunnel), b"last words");
    let record = &rig.records()[0];
    assert_eq!((record.bytes_up, record.bytes_down), (10, 10));
}

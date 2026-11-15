//! Open mode: on the browser listener any public host passes with no allowlist entry, and every
//! other check holds (the block list by name and IP literal with the lawyer's exact allows
//! skipping it, the silent list, the port); `revoke` closes no browser tunnel. The agent
//! listener keeps the allowlist in open mode, and allowlist mode is unchanged.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use clauseo_blocklist::{Allowed, Tag};

use super::blocked::{blocked, current, no_network};
use super::harness::{Rig, Setup, echo, host_denial, origin, read_all, rig, round_trip};
use crate::Denial;
use crate::HostRefusal::{NotAllowed, Port};
use crate::proxy::{ProxyMode, Verdict};

const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";

/// An open-mode rig with no allowlist entry: `anywhere.test` and `evil.example` resolve to
/// 192.0.2.10 (an echo server on 443), `plain.test` to 192.0.2.20 (an origin on 80, whose
/// receiver comes back with the rig).
fn open_rig(setup: Setup) -> (Rig, Receiver<Vec<u8>>) {
    let rig = rig(Setup {
        mode: ProxyMode::Open,
        ..setup
    });
    rig.net.name("anywhere.test", &["192.0.2.10"]);
    rig.net.name("evil.example", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    let (address, received) = origin(OK);
    rig.net.name("plain.test", &["192.0.2.20"]);
    rig.net.route("192.0.2.20:80", address);
    (rig, received)
}

fn allowed(hosts: &[&str]) -> Arc<Allowed> {
    let allowed = Arc::new(Allowed::default());
    allowed.set(hosts.iter().map(|host| (*host).to_owned()).collect());
    allowed
}

#[test]
fn open_mode_passes_a_public_host_on_no_list_over_connect_and_plain_http() {
    let (rig, origin) = open_rig(Setup::default());
    let (mut tunnel, code) = rig.browser_connect("anywhere.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"hi"), b"hi");
    drop(tunnel);
    let (mut literal, code) = rig.browser_connect("192.0.2.10:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut literal, b"ip"), b"ip");
    drop(literal);
    let answer =
        rig.browser_exchange("GET http://plain.test/x HTTP/1.1\r\nHost: plain.test\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert!(answer.ends_with("ok"), "{answer}");
    let got = String::from_utf8(origin.recv().unwrap()).unwrap();
    assert!(
        got.starts_with("GET /x HTTP/1.1\r\nHost: plain.test\r\n"),
        "{got}"
    );
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    // A tunnel is recorded when it ends, so the order of the records is not fixed.
    let mut verdicts: Vec<String> = rig
        .records(3)
        .iter()
        .map(|r| format!("{:?}", r.verdict))
        .collect();
    verdicts.sort();
    assert_eq!(verdicts, ["Forward", "Tunnel", "Tunnel"]);
}

/// Open mode opens the browser listener only: the agent's still needs the allowlist, so a jailed
/// agent never gets open egress.
#[test]
fn open_mode_leaves_the_agent_listener_on_the_allowlist() {
    let (rig, _origin) = open_rig(Setup {
        allow: vec!["listed.test"],
        ..Setup::default()
    });
    rig.net.name("listed.test", &["192.0.2.10"]);
    assert_eq!(rig.connect("anywhere.test:443").1, 403);
    assert_eq!(rig.connect("192.0.2.10:443").1, 403);
    let answer = rig.exchange("GET http://plain.test/ HTTP/1.1\r\nHost: plain.test\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    no_network(&rig);
    let (mut tunnel, code) = rig.connect("listed.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("anywhere.test", 443, NotAllowed, false),
            host_denial("192.0.2.10", 443, NotAllowed, false),
            host_denial("plain.test", 80, NotAllowed, false),
        ]
    );
}

#[test]
fn open_mode_still_refuses_a_listed_host_by_name_and_by_ip_literal() {
    let (rig, _origin) = open_rig(Setup {
        block: current(),
        ..Setup::default()
    });
    for target in [
        "evil.example:443",
        "login.evil.example:443",
        "1.2.3.4:443",
        "[2606:4700:4700::1111]:443",
    ] {
        assert_eq!(rig.browser_connect(target).1, 403, "{target}");
    }
    let answer =
        rig.browser_exchange("GET http://evil.example/ HTTP/1.1\r\nHost: evil.example\r\n\r\n");
    assert!(answer.contains("Clauseo-Blocked: phishing"), "{answer}");
    let answer = rig.browser_exchange("GET http://1.2.3.4/ HTTP/1.1\r\nHost: 1.2.3.4\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    no_network(&rig);
    assert_eq!(
        rig.denials(),
        vec![
            blocked("evil.example", 443, "evil.example", Tag::Phishing),
            blocked("login.evil.example", 443, "evil.example", Tag::Phishing),
            blocked("1.2.3.4", 443, "[1.2.3.4]", Tag::Malware),
            blocked(
                "2606:4700:4700::1111",
                443,
                "[2606:4700:4700::1111]",
                Tag::Malware
            ),
            blocked("evil.example", 80, "evil.example", Tag::Phishing),
            blocked("1.2.3.4", 80, "[1.2.3.4]", Tag::Malware),
        ]
    );
    let records = rig.records(6);
    assert!(
        records.iter().all(|r| r.verdict == Verdict::Blocked),
        "{records:?}"
    );
}

#[test]
fn open_mode_lets_the_lawyer_s_exact_allow_skip_the_list() {
    let (rig, _origin) = open_rig(Setup {
        block: current(),
        allowed: allowed(&["evil.example"]),
        ..Setup::default()
    });
    let (mut tunnel, code) = rig.browser_connect("evil.example:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    // Exactly the host: a name under it is still listed.
    assert_eq!(rig.browser_connect("login.evil.example:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![blocked(
            "login.evil.example",
            443,
            "evil.example",
            Tag::Phishing
        )]
    );
}

#[test]
fn open_mode_keeps_the_silent_list_and_the_port_rule() {
    let (rig, _origin) = open_rig(Setup {
        silent: vec!["telemetry.test"],
        ..Setup::default()
    });
    rig.net.name("telemetry.test", &["192.0.2.10"]);
    assert_eq!(rig.browser_connect("telemetry.test:443").1, 403);
    assert_eq!(rig.browser_connect("anywhere.test:8443").1, 403);
    let answer =
        rig.browser_exchange("GET http://plain.test:8080/ HTTP/1.1\r\nHost: plain.test\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert_eq!(rig.net.dialled(), Vec::new());
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("anywhere.test", 8443, Port, false),
            host_denial("plain.test", 8080, Port, false),
        ]
    );
    let records = rig.records(3);
    assert!(
        records.iter().any(|r| r.verdict == Verdict::Silent),
        "{records:?}"
    );
}

#[test]
fn allowlist_mode_still_refuses_a_public_host_off_the_list() {
    let rig = rig(Setup::default());
    rig.net.name("anywhere.test", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    assert_eq!(rig.connect("anywhere.test:443").1, 403);
    assert_eq!(rig.browser_connect("192.0.2.10:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("anywhere.test", 443, NotAllowed, false),
            host_denial("192.0.2.10", 443, NotAllowed, false),
        ]
    );
    no_network(&rig);
}

#[test]
fn revoke_in_open_mode_closes_no_browser_tunnel_and_refuses_nothing() {
    let (rig, _origin) = open_rig(Setup::default());
    let (mut tunnel, code) = rig.browser_connect("anywhere.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"one"), b"one");
    rig.handle.allow("anywhere.test");
    rig.handle.revoke("anywhere.test");
    assert_eq!(
        round_trip(&mut tunnel, b"two"),
        b"two",
        "the tunnel stays open"
    );
    let (mut next, code) = rig.browser_connect("anywhere.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut next, b"three"), b"three");
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

/// The block list's revoke watcher works in open mode too: a tunnel to a listed host the lawyer
/// allowed closes when that allow is revoked.
#[test]
fn revoking_the_lawyer_s_allow_in_open_mode_closes_a_listed_host_s_tunnel() {
    let live = allowed(&["evil.example"]);
    let (rig, _origin) = open_rig(Setup {
        block: current(),
        allowed: Arc::clone(&live),
        ..Setup::default()
    });
    let (mut other, code) = rig.browser_connect("anywhere.test:443");
    assert_eq!(code, 200);
    let (mut tunnel, code) = rig.browser_connect("evil.example:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    live.set(BTreeSet::new());
    assert_eq!(read_all(&mut tunnel), b"", "the revoke closes the tunnel");
    assert_eq!(round_trip(&mut other, b"still"), b"still");
    assert_eq!(rig.browser_connect("evil.example:443").1, 403);
}

/// In open mode a revoke still closes the agent's tunnels to the host (the agent listener keeps
/// the list), and leaves the browser's to the same host open.
#[test]
fn revoke_in_open_mode_closes_the_agent_s_tunnel_and_not_the_browser_s() {
    let (rig, _origin) = open_rig(Setup {
        allow: vec!["anywhere.test"],
        ..Setup::default()
    });
    let (mut agent, code) = rig.connect("anywhere.test:443");
    assert_eq!(code, 200);
    let (mut browser, code) = rig.browser_connect("anywhere.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut agent, b"a"), b"a");
    assert_eq!(round_trip(&mut browser, b"b"), b"b");
    rig.handle.revoke("anywhere.test");
    assert_eq!(read_all(&mut agent), b"", "the agent's tunnel closes");
    assert_eq!(
        round_trip(&mut browser, b"c"),
        b"c",
        "the browser's stays open"
    );
    assert_eq!(rig.connect("anywhere.test:443").1, 403);
    assert_eq!(rig.browser_connect("anywhere.test:443").1, 200);
}

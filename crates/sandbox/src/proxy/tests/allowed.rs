//! The lawyer's allowed sites (ADR 0053, PR 2): asked before the block list, exactly by host;
//! an allowed host then meets the silent list, the port, the private-address rule and the
//! allowlist like any host, and a change reaches the next request; a revoke closes the open
//! tunnels and forwards to a host the list still names.

use std::collections::BTreeSet;
use std::io::{Read as _, Write as _};
use std::sync::{Arc, mpsc};

use clauseo_blocklist::{Allowed, Tag};

use super::blocked::{blocked, current, no_network};
use super::harness::{
    Rig, Setup, echo, host_denial, origin, read_all, read_head, rig, round_trip, serve, status,
};
use crate::Denial;
use crate::HostRefusal::{NotAllowed, Port, PrivateAddress};

fn allowed(hosts: &[&str]) -> Arc<Allowed> {
    let allowed = Arc::new(Allowed::default());
    allowed.set(hosts.iter().map(|host| (*host).to_owned()).collect());
    allowed
}

/// A rig with the list loaded and `allowed`, allowing `allow`, each resolving to 192.0.2.10,
/// where an echo server answers on 443.
fn allowed_rig(allow: &[&'static str], silent: &[&'static str], allowed: Arc<Allowed>) -> Rig {
    let rig = rig(Setup {
        allow: allow.to_vec(),
        silent: silent.to_vec(),
        block: current(),
        allowed,
        ..Setup::default()
    });
    for host in allow {
        rig.net.name(host, &["192.0.2.10"]);
    }
    rig.net.route("192.0.2.10:443", echo());
    rig
}

#[test]
fn an_allowed_host_passes_the_list_and_still_needs_the_allowlist() {
    let rig = allowed_rig(&["evil.example"], &[], allowed(&["evil.example"]));
    let (mut tunnel, code) = rig.connect("evil.example:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    assert_eq!(rig.denials(), Vec::<Denial>::new());

    // Not on the allowlist: refused as any host is, not as a listed one.
    let rig = allowed_rig(&[], &[], allowed(&["evil.example"]));
    assert_eq!(rig.connect("evil.example:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![host_denial("evil.example", 443, NotAllowed, false)]
    );
}

#[test]
fn an_allow_covers_the_exact_host_only() {
    let rig = allowed_rig(
        &["evil.example", "login.evil.example"],
        &[],
        allowed(&["evil.example"]),
    );
    assert_eq!(rig.connect("login.evil.example:443").1, 403);
    let denial = blocked("login.evil.example", 443, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
}

#[test]
fn the_silent_list_still_refuses_an_allowed_host() {
    let rig = allowed_rig(
        &["evil.example"],
        &["evil.example"],
        allowed(&["evil.example"]),
    );
    assert_eq!(rig.connect("evil.example:443").1, 403);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    no_network(&rig);
}

#[test]
fn an_allowed_host_meets_the_port_and_private_address_rules() {
    let rig = allowed_rig(
        &["evil.example", "10.0.0.1"],
        &[],
        allowed(&["evil.example", "10.0.0.1"]),
    );
    assert_eq!(rig.connect("evil.example:8443").1, 403);
    assert_eq!(rig.connect("10.0.0.1:443").1, 403);
    no_network(&rig);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("evil.example", 8443, Port, false),
            host_denial("10.0.0.1", 443, PrivateAddress, false),
        ]
    );
}

#[test]
fn a_listed_host_header_is_refused_beside_an_allowed_target() {
    let rig = allowed_rig(&["1.2.3.4"], &[], allowed(&["1.2.3.4"]));
    let answer = rig.exchange("GET http://1.2.3.4/ HTTP/1.1\r\nHost: evil.example\r\n\r\n");
    assert!(answer.contains("Clauseo-Blocked: phishing"), "{answer}");
    no_network(&rig);
    let denial = blocked("evil.example", 80, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
}

/// An allow reaches the next request; its revoke closes the tunnel opened while allowed, since
/// the list still names the host, and refuses the next request.
#[test]
fn an_allow_and_its_revoke_reach_the_next_request() {
    let live = allowed(&[]);
    let rig = allowed_rig(&["evil.example"], &[], Arc::clone(&live));
    assert_eq!(rig.connect("evil.example:443").1, 403);
    live.set(BTreeSet::from(["evil.example".to_owned()]));
    let (mut tunnel, code) = rig.connect("evil.example:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    live.set(BTreeSet::new());
    assert_eq!(read_all(&mut tunnel), b"", "the revoke closes the tunnel");
    assert_eq!(rig.connect("evil.example:443").1, 403);
    let denial = blocked("evil.example", 443, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial.clone(), denial]);
}

/// Plain `http://` asks the same allowed set: an allowed listed host on the allowlist is
/// forwarded to its origin, and without the allow it is refused as listed.
#[test]
fn an_allowed_host_passes_over_plain_http_too() {
    const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
    let request = "GET http://evil.example/x HTTP/1.1\r\nHost: evil.example\r\n\r\n";
    let rig = allowed_rig(&["evil.example"], &[], allowed(&["evil.example"]));
    let (address, received) = origin(OK);
    rig.net.route("192.0.2.10:80", address);
    assert_eq!(rig.exchange(request), OK);
    assert!(received.recv().is_ok(), "the origin got the request");
    assert_eq!(rig.denials(), Vec::<Denial>::new());

    let rig = allowed_rig(&["evil.example"], &[], allowed(&[]));
    let answer = rig.exchange(request);
    assert!(answer.contains("Clauseo-Blocked: phishing"), "{answer}");
    no_network(&rig);
    let denial = blocked("evil.example", 80, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
}

/// A revoke while a plain `http://` request to the host is being forwarded closes it: the
/// origin sees the proxy hang up and the client gets no answer.
#[test]
fn a_revoke_closes_an_open_plain_http_forward() {
    let live = allowed(&["evil.example"]);
    let rig = allowed_rig(&["evil.example"], &[], Arc::clone(&live));
    let (arrived, got) = mpsc::channel();
    let (closed, hung_up) = mpsc::channel();
    let (arrived, closed) = (
        std::sync::Mutex::new(arrived),
        std::sync::Mutex::new(closed),
    );
    // Reads the head, then never answers: it waits for the proxy to close the connection.
    let address = serve(move |mut stream| {
        read_head(&mut stream);
        arrived.lock().unwrap().send(()).unwrap();
        let mut rest = Vec::new();
        drop(stream.read_to_end(&mut rest));
        closed.lock().unwrap().send(()).unwrap();
    });
    rig.net.route("192.0.2.10:80", address);
    let mut stream = rig.agent();
    let request = format!(
        "GET http://evil.example/x HTTP/1.1\r\nHost: evil.example\r\n{}\r\n",
        rig.auth()
    );
    stream.write_all(request.as_bytes()).unwrap();
    got.recv().unwrap();
    live.set(BTreeSet::new());
    assert_eq!(read_all(&mut stream), b"", "closed with no answer");
    hung_up.recv().unwrap();
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

/// A revoke that lands after a request passed the list but before its tunnel opens (here,
/// during DNS) refuses it as listed, so the revoke's sweep cannot miss it.
#[test]
fn a_revoke_during_dns_refuses_the_connect_as_listed() {
    let live = allowed(&["evil.example"]);
    let rig = allowed_rig(&["evil.example"], &[], Arc::clone(&live));
    let held = rig.net.hold_next_resolve();
    let mut stream = rig.agent();
    let request = format!("CONNECT evil.example:443 HTTP/1.1\r\n{}\r\n", rig.auth());
    stream.write_all(request.as_bytes()).unwrap();
    held.arrived.recv().unwrap();
    live.set(BTreeSet::new());
    held.release.send(()).unwrap();
    assert_eq!(status(&read_head(&mut stream)), 403);
    assert_eq!(rig.net.dialled(), Vec::<std::net::SocketAddr>::new());
    let denial = blocked("evil.example", 443, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
}

/// An IPv6 allow is stored as `Name::text` prints it, without brackets, and lets a CONNECT to
/// the bracketed literal past the list; a bracketed entry matches nothing.
#[test]
fn an_ipv6_allow_without_brackets_passes_the_bracketed_literal() {
    let v6 = "2606:4700:4700::1111";
    let rig = allowed_rig(&["[2606:4700:4700::1111]"], &[], allowed(&[v6]));
    rig.net.route("[2606:4700:4700::1111]:443", echo());
    let (mut tunnel, code) = rig.connect("[2606:4700:4700::1111]:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    assert_eq!(rig.denials(), Vec::<Denial>::new());

    let rig = allowed_rig(&[], &[], allowed(&["[2606:4700:4700::1111]"]));
    assert_eq!(rig.connect("[2606:4700:4700::1111]:443").1, 403);
    let denial = blocked(v6, 443, &format!("[{v6}]"), Tag::Malware);
    assert_eq!(rig.denials(), vec![denial]);
}

/// Case, one trailing dot and a Unicode IDN are folded before the allow is asked, so each
/// spelling of the allowed punycode host passes.
#[test]
fn other_spellings_of_an_allowed_host_pass() {
    let idn = "xn--80ak6aa92e.com";
    let rig = allowed_rig(&["evil.example", idn], &[], allowed(&["evil.example", idn]));
    for target in ["EVIL.Example:443", "evil.example.:443", "аррӏе.com:443"] {
        let (mut tunnel, code) = rig.connect(target);
        assert_eq!(code, 200, "{target}");
        assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok", "{target}");
    }
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

/// An allow for a host under a listed one does not cover the listed parent.
#[test]
fn an_allowed_child_leaves_its_parent_blocked() {
    let rig = allowed_rig(&["evil.example"], &[], allowed(&["login.evil.example"]));
    assert_eq!(rig.connect("evil.example:443").1, 403);
    let denial = blocked("evil.example", 443, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
}

/// Against an allow for `1.2.3.4`: the integer spelling is read as 1.2.3.4 and passes; the
/// IPv4-mapped and NAT64 IPv6 forms keep their own text, so the allow misses them, and the list
/// still catches them through the IPv4 address they carry.
#[test]
fn ipv6_forms_of_an_allowed_ipv4_stay_blocked() {
    let rig = allowed_rig(&["1.2.3.4"], &[], allowed(&["1.2.3.4"]));
    rig.net.route("1.2.3.4:443", echo());
    let (mut tunnel, code) = rig.connect("16909060:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    assert_eq!(rig.connect("[::ffff:1.2.3.4]:443").1, 403);
    assert_eq!(rig.connect("[64:ff9b::102:304]:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![
            blocked("::ffff:1.2.3.4", 443, "[1.2.3.4]", Tag::Malware),
            blocked("64:ff9b::102:304", 443, "[1.2.3.4]", Tag::Malware),
        ]
    );
}

/// The browser listener, which takes no token, asks the same allowed set.
#[test]
fn the_browser_listener_passes_an_allowed_host() {
    let rig = rig(Setup {
        allow: vec!["evil.example"],
        block: current(),
        allowed: allowed(&["evil.example"]),
        browser: true,
        ..Setup::default()
    });
    rig.net.name("evil.example", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    let mut stream = rig.browser();
    stream
        .write_all(b"CONNECT evil.example:443 HTTP/1.1\r\nHost: evil.example\r\n\r\n")
        .unwrap();
    assert_eq!(status(&read_head(&mut stream)), 200);
    assert_eq!(round_trip(&mut stream, b"ok"), b"ok");
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

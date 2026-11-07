//! The block list: refused before DNS whatever the allowlist says, after the silent list, by
//! name, parent domain or IP literal, and a reload that reaches the next request only.

use std::io::Write;
use std::sync::Arc;

use clauseo_blocklist::{BlockList, Current, Generated, Header, Tag, parse_plain};

use super::harness::{Rig, Setup, echo, host_denial, read_head, rig, round_trip, status};
use crate::Denial;
use crate::HostRefusal::NotAllowed;
use crate::proxy::Verdict;

const MADE: &str = "2026-11-07T06:00:00Z";
const ENTRIES: &[&str] = &[
    "1.2.3.4 m",
    "10.0.0.1 m",
    "198.51.100.7 m",
    "[2606:4700:4700::1111] m",
    "evil.example p",
    "xn--80ak6aa92e.com p",
];

/// A list made at `made` with `entries` (`<host> <tag>` lines, in any order).
fn list(made: &str, entries: &[&str]) -> BlockList {
    let mut lines = entries.to_vec();
    lines.sort_unstable();
    let mut text = lines.join("\n");
    text.push('\n');
    let header = Header {
        generated: Generated::parse(made).unwrap(),
        entries: u32::try_from(lines.len()).unwrap(),
    };
    parse_plain(text.as_bytes(), header).unwrap()
}

fn current() -> Arc<Current> {
    Arc::new(Current::new(list(MADE, ENTRIES)))
}

/// A rig with the list loaded, allowing `allow`, each resolving to 192.0.2.10, where an echo
/// server answers on 443.
fn block_rig(allow: &[&'static str], silent: &[&'static str]) -> Rig {
    let rig = rig(Setup {
        allow: allow.to_vec(),
        silent: silent.to_vec(),
        block: current(),
        ..Setup::default()
    });
    for host in allow {
        rig.net.name(host, &["192.0.2.10"]);
    }
    rig.net.route("192.0.2.10:443", echo());
    rig
}

fn blocked(host: &str, port: u16, listed: &str, tag: Tag) -> Denial {
    blocked_at(host, port, listed, tag, MADE)
}

fn blocked_at(host: &str, port: u16, listed: &str, tag: Tag, made: &str) -> Denial {
    Denial::Blocked {
        host: host.to_owned(),
        port,
        listed: listed.to_owned(),
        tag,
        generated: Generated::parse(made).unwrap(),
    }
}

/// A CONNECT's answer head.
fn connect_head(rig: &Rig, target: &str) -> String {
    let mut stream = rig.agent();
    let request = format!("CONNECT {target} HTTP/1.1\r\n{}\r\n", rig.auth());
    stream.write_all(request.as_bytes()).unwrap();
    read_head(&mut stream)
}

/// Neither DNS nor a connect happened.
fn no_network(rig: &Rig) {
    assert_eq!(rig.net.resolved(), Vec::<String>::new());
    assert_eq!(rig.net.dialled(), Vec::new());
}

#[test]
fn a_listed_host_is_refused_over_connect_with_a_plain_403_and_no_dns() {
    let rig = block_rig(&[], &[]);
    let head = connect_head(&rig, "evil.example:443");
    assert_eq!(status(&head), 403);
    assert!(!head.contains("Clauseo-Blocked"), "{head}");
    no_network(&rig);
    let denial = blocked("evil.example", 443, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
    let record = &rig.records(1)[0];
    assert_eq!((record.verdict, record.why), (Verdict::Blocked, None));
    assert_eq!((record.host.as_str(), record.port), ("evil.example", 443));
}

#[test]
fn a_listed_host_is_refused_over_plain_http_with_the_tag_header() {
    let rig = block_rig(&[], &[]);
    let answer = rig.exchange("GET http://evil.example/x HTTP/1.1\r\nHost: evil.example\r\n\r\n");
    assert_eq!(status(&answer), 403, "{answer}");
    assert!(
        answer.contains("\r\nClauseo-Blocked: phishing\r\n"),
        "{answer}"
    );
    no_network(&rig);
    let denial = blocked("evil.example", 80, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
    assert_eq!(rig.records(1)[0].verdict, Verdict::Blocked);
}

#[test]
fn a_subdomain_is_refused_and_a_lookalike_is_not() {
    let rig = block_rig(&[], &[]);
    assert_eq!(rig.connect("login.evil.example:443").1, 403);
    assert_eq!(rig.connect("evil-evil.example:443").1, 403);
    assert_eq!(rig.connect("notevil.example:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![
            blocked("login.evil.example", 443, "evil.example", Tag::Phishing),
            host_denial("evil-evil.example", 443, NotAllowed, false),
            host_denial("notevil.example", 443, NotAllowed, false),
        ]
    );
}

#[test]
fn case_a_trailing_dot_and_an_idn_are_read_before_the_list() {
    let rig = block_rig(&[], &[]);
    assert_eq!(rig.connect("EVIL.example.:443").1, 403);
    let answer = rig.exchange("GET http://аррӏе.com/ HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(answer.contains("Clauseo-Blocked: phishing"), "{answer}");
    assert_eq!(rig.connect("XN--80AK6AA92E.COM:443").1, 403);
    no_network(&rig);
    assert_eq!(
        rig.denials(),
        vec![
            blocked("evil.example", 443, "evil.example", Tag::Phishing),
            blocked(
                "xn--80ak6aa92e.com",
                80,
                "xn--80ak6aa92e.com",
                Tag::Phishing
            ),
            blocked(
                "xn--80ak6aa92e.com",
                443,
                "xn--80ak6aa92e.com",
                Tag::Phishing
            ),
        ]
    );
}

#[test]
fn listed_ip_literals_are_refused_over_connect_and_plain_http() {
    let rig = block_rig(&[], &[]);
    assert_eq!(rig.connect("1.2.3.4:443").1, 403);
    assert_eq!(rig.connect("[2606:4700:4700::1111]:443").1, 403);
    for url in ["http://1.2.3.4/", "http://[2606:4700:4700::1111]/"] {
        let answer = rig.exchange(&format!("GET {url} HTTP/1.1\r\nHost: x\r\n\r\n"));
        assert!(answer.contains("Clauseo-Blocked: malware"), "{answer}");
    }
    no_network(&rig);
    let v6 = "2606:4700:4700::1111";
    assert_eq!(
        rig.denials(),
        vec![
            blocked("1.2.3.4", 443, "[1.2.3.4]", Tag::Malware),
            blocked(v6, 443, &format!("[{v6}]"), Tag::Malware),
            blocked("1.2.3.4", 80, "[1.2.3.4]", Tag::Malware),
            blocked(v6, 80, &format!("[{v6}]"), Tag::Malware),
        ]
    );
}

#[test]
fn a_listed_host_header_refuses_a_plain_http_request_to_an_allowed_host() {
    let rig = block_rig(&["allowed.test"], &[]);
    let answer = rig.exchange("GET http://allowed.test/ HTTP/1.1\r\nHost: Evil.Example.\r\n\r\n");
    assert!(answer.contains("Clauseo-Blocked: phishing"), "{answer}");
    no_network(&rig);
    let denial = blocked("evil.example", 80, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
}

#[test]
fn the_block_list_beats_an_allowlist_entry() {
    let rig = block_rig(&["evil.example"], &[]);
    assert_eq!(rig.connect("evil.example:443").1, 403);
    no_network(&rig);
    let denial = blocked("evil.example", 443, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
}

#[test]
fn the_silent_list_beats_the_block_list() {
    let rig = block_rig(&[], &["evil.example"]);
    assert_eq!(rig.connect("evil.example:443").1, 403);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
    assert_eq!(rig.records(1)[0].verdict, Verdict::Silent);
}

#[test]
fn an_allowed_host_not_on_the_list_passes() {
    let rig = block_rig(&["fine.test"], &[]);
    let (mut tunnel, code) = rig.connect("fine.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

#[test]
fn with_no_list_loaded_the_allowlist_decides_and_the_notice_is_taken_once() {
    let block = Arc::new(Current::default());
    let rig = rig(Setup {
        allow: vec!["evil.example"],
        block: Arc::clone(&block),
        ..Setup::default()
    });
    assert!(
        !block.first_unloaded_notice(),
        "the proxy took the notice at start"
    );
    rig.net.name("evil.example", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    let (mut tunnel, code) = rig.connect("evil.example:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"ok"), b"ok");
    assert_eq!(rig.connect("other.test:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![host_denial("other.test", 443, NotAllowed, false)]
    );
}

#[test]
fn a_reload_keeps_an_open_tunnel_and_refuses_the_next_connect() {
    let block = current();
    let rig = rig(Setup {
        allow: vec!["newer.test"],
        block: Arc::clone(&block),
        ..Setup::default()
    });
    rig.net.name("newer.test", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    let (mut tunnel, code) = rig.connect("newer.test:443");
    assert_eq!(code, 200);
    let later = "2026-11-07T12:00:00Z";
    let newer = list(later, &[ENTRIES, &["newer.test p"]].concat());
    assert!(block.offer(newer, None));
    assert_eq!(round_trip(&mut tunnel, b"still"), b"still");
    assert_eq!(rig.connect("newer.test:443").1, 403);
    assert_eq!(round_trip(&mut tunnel, b"open"), b"open");
    let denial = blocked_at("newer.test", 443, "newer.test", Tag::Phishing, later);
    assert_eq!(rig.denials(), vec![denial]);
}

#[test]
fn the_rate_cap_holds_blocked_denials_like_the_others() {
    let rig = block_rig(&[], &[]);
    for _ in 0..25 {
        assert_eq!(rig.connect("evil.example:443").1, 403);
    }
    assert_eq!(rig.denials().len(), 20);
    rig.clock.advance(crate::DENIAL_WINDOW);
    assert_eq!(rig.connect("evil.example:443").1, 403);
    let denials = rig.denials();
    assert_eq!(denials.len(), 22);
    assert_eq!(denials[20], Denial::Dropped { count: 5 });
    let denial = blocked("evil.example", 443, "evil.example", Tag::Phishing);
    assert_eq!(denials[21], denial);
}

#[test]
fn the_block_list_is_asked_before_the_port_rule() {
    let rig = block_rig(&[], &[]);
    assert_eq!(rig.connect("evil.example:8443").1, 403);
    let denial = blocked("evil.example", 8443, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
}

#[test]
fn the_block_list_is_asked_before_the_private_address_rule_and_the_allowlist() {
    let rig = block_rig(&["198.51.100.7"], &[]);
    assert_eq!(rig.connect("10.0.0.1:443").1, 403);
    assert_eq!(rig.connect("198.51.100.7:443").1, 403);
    no_network(&rig);
    assert_eq!(
        rig.denials(),
        vec![
            blocked("10.0.0.1", 443, "[10.0.0.1]", Tag::Malware),
            blocked("198.51.100.7", 443, "[198.51.100.7]", Tag::Malware),
        ]
    );
}

#[test]
fn the_browser_listener_refuses_a_listed_host_too() {
    let rig = block_rig(&["evil.example"], &[]);
    let mut stream = rig.browser();
    stream
        .write_all(b"CONNECT evil.example:443 HTTP/1.1\r\nHost: evil.example\r\n\r\n")
        .unwrap();
    assert_eq!(status(&read_head(&mut stream)), 403);
    no_network(&rig);
    let denial = blocked("evil.example", 443, "evil.example", Tag::Phishing);
    assert_eq!(rig.denials(), vec![denial]);
    assert_eq!(rig.records(1)[0].verdict, Verdict::Blocked);
}

/// The URL parser reads every IPv4 spelling (decimal, hex, octal) and an IPv4-mapped IPv6
/// address as the address itself, and decodes `%2E` in a host; each is matched as 1.2.3.4 or
/// `evil.example` and refused with the tag header.
#[test]
fn other_spellings_of_a_listed_ip_or_host_over_plain_http_are_refused() {
    let rig = block_rig(&[], &[]);
    let urls = [
        ("http://16909060/", "1.2.3.4"),
        ("http://0x01020304/", "1.2.3.4"),
        ("http://0001.0002.0003.0004/", "1.2.3.4"),
        ("http://[::ffff:1.2.3.4]/", "::ffff:1.2.3.4"),
        ("http://evil%2Eexample/", "evil.example"),
    ];
    for (url, _) in urls {
        let answer = rig.exchange(&format!("GET {url} HTTP/1.1\r\nHost: x\r\n\r\n"));
        assert_eq!(status(&answer), 403, "{url}: {answer}");
        assert!(answer.contains("Clauseo-Blocked: "), "{url}: {answer}");
    }
    no_network(&rig);
    let denials = rig.denials();
    assert_eq!(denials.len(), urls.len(), "{denials:?}");
    let hosts: Vec<String> = denials
        .into_iter()
        .filter_map(|denial| {
            if let Denial::Blocked { host, .. } = denial {
                Some(host)
            } else {
                None
            }
        })
        .collect();
    let want: Vec<&str> = urls.iter().map(|(_, host)| *host).collect();
    assert_eq!(hosts, want);
}

/// Userinfo in a URL or a CONNECT authority is refused with 400 by the target check, before
/// the block list is asked: still refused, with no denial and no network.
#[test]
fn userinfo_with_a_listed_host_is_refused_before_the_list() {
    let rig = block_rig(&[], &[]);
    let answer = rig.exchange("GET http://user@evil.example/ HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(status(&answer), 400, "{answer}");
    assert_eq!(rig.connect("user@evil.example:443").1, 400);
    no_network(&rig);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

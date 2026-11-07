//! Matching a request host: exact, subdomain, lookalike, IP literals, case and trailing dot.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use clauseo_blocklist::{BlockList, Tag};

use crate::fixtures::plain;

fn list() -> BlockList {
    plain(&[
        "1.2.3.4 m",
        "[2001:db8::1] m",
        "evil.example p",
        "login.bank.test p",
        "xn--80ak6aa92e.com p",
    ])
}

fn listed(host: &str) -> Option<&'static str> {
    // Leaks one small list per call; tests only.
    let list: &'static BlockList = Box::leak(Box::new(list()));
    list.lookup(host).map(|hit| hit.listed)
}

#[test]
fn an_exact_host_matches() {
    assert_eq!(listed("evil.example"), Some("evil.example"));
    assert_eq!(listed("login.bank.test"), Some("login.bank.test"));
}

#[test]
fn a_subdomain_matches_on_label_boundaries() {
    assert_eq!(listed("a.evil.example"), Some("evil.example"));
    assert_eq!(listed("a.b.c.evil.example"), Some("evil.example"));
    assert_eq!(listed("x.login.bank.test"), Some("login.bank.test"));
}

#[test]
fn a_lookalike_or_a_parent_does_not_match() {
    for host in [
        "evil-evil.example",
        "notevil.example",
        "evil.example.com",
        "example",
        "bank.test",
        "ogin.bank.test",
        "evil.examples",
    ] {
        assert_eq!(listed(host), None, "{host}");
    }
}

#[test]
fn case_and_one_trailing_dot_from_the_request_are_folded() {
    assert_eq!(listed("EVIL.Example"), Some("evil.example"));
    assert_eq!(listed("a.evil.example."), Some("evil.example"));
    assert_eq!(listed("A.EVIL.EXAMPLE."), Some("evil.example"));
    assert_eq!(listed("evil.example.."), None);
}

#[test]
fn an_idn_matches_in_its_ascii_form() {
    assert_eq!(listed("www.xn--80ak6aa92e.com"), Some("xn--80ak6aa92e.com"));
}

#[test]
fn ipv4_literals_match_exactly_and_only_as_addresses() {
    assert_eq!(listed("1.2.3.4"), Some("[1.2.3.4]"));
    assert_eq!(listed("1.2.3.4."), Some("[1.2.3.4]"));
    assert_eq!(listed("1.2.3.5"), None);
    assert_eq!(listed("x.1.2.3.4"), None);
    assert_eq!(listed("2.3.4"), None);
    let list = list();
    let hit = list
        .lookup_ip(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)))
        .unwrap();
    assert_eq!(hit.tag, Tag::Malware);
    // An IPv6 address that carries the IPv4 address matches its entry, in each form the proxy's
    // address check reads: mapped, translated, compatible, NAT64 and 6to4.
    for carried in [
        "[::ffff:1.2.3.4]",
        "[::ffff:0:1.2.3.4]",
        "[::1.2.3.4]",
        "[64:ff9b::102:304]",
        "[2002:102:304::1]",
    ] {
        assert_eq!(
            list.lookup(carried).map(|hit| hit.listed),
            Some("[1.2.3.4]"),
            "{carried}"
        );
    }
    assert_eq!(list.lookup("[64:ff9b:1::102:304]"), None);
}

#[test]
fn ipv6_literals_match_exactly_in_any_spelling() {
    let list = list();
    for host in [
        "[2001:db8::1]",
        "2001:db8::1",
        "[2001:0DB8:0:0::1]",
        "[2001:db8:0:0:0:0:0:1]",
    ] {
        assert_eq!(
            list.lookup(host).map(|hit| hit.listed),
            Some("[2001:db8::1]"),
            "{host}"
        );
    }
    assert_eq!(list.lookup("[2001:db8::2]"), None);
    let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
    assert_eq!(list.lookup_ip(ip).unwrap().tag, Tag::Malware);
}

#[test]
fn the_empty_list_matches_nothing() {
    let list = BlockList::empty();
    assert!(list.is_unloaded());
    assert_eq!(list.lookup("evil.example"), None);
    assert_eq!(list.lookup_ip(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))), None);
}

#[test]
fn every_entry_of_a_larger_list_is_found_and_its_neighbours_are_not() {
    let hosts: Vec<String> = (0..5000).map(|n| format!("h{n:05}.example")).collect();
    let lines: Vec<String> = hosts.iter().map(|h| format!("{h} p")).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let list = plain(&refs);
    for host in &hosts {
        assert_eq!(
            list.lookup(&format!("www.{host}")).map(|h| h.listed),
            Some(host.as_str())
        );
        assert_eq!(list.lookup(&format!("x{host}")), None);
    }
}

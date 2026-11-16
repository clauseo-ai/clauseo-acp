//! Open mode's name-only path (`name_only`), what it still refuses: single labels, the internal
//! suffixes and the firm's own; IP literals in every form, checked by the proxy itself; listed
//! hosts, by name before any DNS. And the launch's skip of local DNS after three failures.

use std::net::SocketAddr;
use std::time::Duration;

use clauseo_blocklist::Tag;

use super::blocked::{blocked, current};
use super::harness::{Setup, host_denial, rig, upstream};
use super::open_upstream::{OPEN, through};
use crate::HostRefusal::{DnsUnavailable, PrivateAddress};
use crate::Upstream;
use crate::proxy::ProxyMode;

#[test]
fn the_name_only_path_refuses_single_labels_internal_and_firm_names() {
    let (address, heads) = upstream(OPEN);
    let rig = through(address, ProxyMode::Open, &[], &["firm.example"]);
    let refused = [
        "intranet",
        "wiki.corp",
        "printer.local",
        "nas.home.arpa",
        "box.lan",
        "svc.internal",
        "pc.localdomain",
        "portal.intranet",
        "firm.example",
        "dms.emea.firm.example",
    ];
    for host in refused {
        assert_eq!(rig.browser_connect(&format!("{host}:443")).1, 403, "{host}");
    }
    let denials: Vec<_> = refused
        .iter()
        .map(|host| host_denial(host, 443, DnsUnavailable, false))
        .collect();
    assert_eq!(rig.denials(), denials);
    assert_eq!(heads.try_iter().count(), 0, "nothing upstream");
    // A public name on the same launch still goes on by name.
    assert_eq!(rig.browser_connect("www.example.com:443").1, 200);
    let head = heads.recv().unwrap();
    assert!(head.starts_with("CONNECT www.example.com:443 "), "{head}");
}

#[test]
fn ip_literals_in_every_form_are_refused_before_the_firm_s_proxy() {
    let (address, heads) = upstream(OPEN);
    let rig = through(address, ProxyMode::Open, &[], &[]);
    for target in [
        "2130706433:443",
        "0177.0.0.1:443",
        "0x7f.1:443",
        "[::ffff:127.0.0.1]:443",
    ] {
        assert_eq!(rig.browser_connect(target).1, 403, "{target}");
    }
    let loopback = host_denial("127.0.0.1", 443, PrivateAddress, false);
    assert_eq!(
        rig.denials(),
        vec![
            loopback.clone(),
            loopback.clone(),
            loopback,
            host_denial("::ffff:127.0.0.1", 443, PrivateAddress, false),
        ]
    );
    assert_eq!(heads.try_iter().count(), 0, "nothing upstream");
    assert_eq!(rig.net.resolved(), Vec::<String>::new());
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn the_block_list_still_answers_403_by_name_before_any_lookup() {
    let (address, heads) = upstream(OPEN);
    let url = url::Url::parse(&format!("http://{address}")).unwrap();
    let rig = rig(Setup {
        mode: ProxyMode::Open,
        block: current(),
        upstream: Upstream::Fixed(url),
        ..Setup::default()
    });
    assert_eq!(rig.browser_connect("evil.example:443").1, 403);
    assert_eq!(rig.browser_connect("www.evil.example:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![
            blocked("evil.example", 443, "evil.example", Tag::Phishing),
            blocked("www.evil.example", 443, "evil.example", Tag::Phishing),
        ]
    );
    assert_eq!(rig.net.resolved(), Vec::<String>::new(), "no DNS");
    assert_eq!(heads.try_iter().count(), 0, "nothing upstream");
}

#[test]
fn three_failed_lookups_in_a_row_skip_local_dns_for_60_s() {
    let (address, heads) = upstream(OPEN);
    let rig = through(address, ProxyMode::Open, &[], &[]);
    rig.net.name("inside.test", &["10.0.0.9"]);
    let code = |host: &str| rig.browser_connect(&format!("{host}:443")).1;
    // Two failures, then an answer: the count starts again.
    assert_eq!(code("a.test"), 200);
    assert_eq!(code("b.test"), 200);
    assert_eq!(code("inside.test"), 403);
    assert_eq!(code("c.test"), 200);
    assert_eq!(code("d.test"), 200);
    assert_eq!(code("e.test"), 200, "the third failure in a row");
    let asked = [
        "a.test",
        "b.test",
        "inside.test",
        "c.test",
        "d.test",
        "e.test",
    ];
    assert_eq!(rig.net.resolved(), asked);
    // Skipped: no lookup, so the name checks alone decide. A private local answer goes unseen:
    // the cost of the skip, on a network whose DNS has just failed three times.
    assert_eq!(code("wiki.corp"), 403);
    assert_eq!(code("inside.test"), 200);
    rig.clock.advance(Duration::from_secs(59));
    assert_eq!(code("f.test"), 200);
    assert_eq!(rig.net.resolved(), asked, "no lookup for 60 s");
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(code("inside.test"), 403, "asked again");
    assert_eq!(rig.net.resolved().len(), asked.len() + 1);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("inside.test", 443, PrivateAddress, false),
            host_denial("wiki.corp", 443, DnsUnavailable, false),
            host_denial("inside.test", 443, PrivateAddress, false),
        ]
    );
    assert_eq!(
        heads.try_iter().count(),
        7,
        "a to f, and inside.test while skipped"
    );
}

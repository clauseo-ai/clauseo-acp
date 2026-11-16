//! Open mode's name-only path (`name_only`), what it still refuses: single labels, the internal
//! suffixes and the firm's own, names that spell an address, the firm proxy's own host; IP
//! literals in every form, checked by the proxy itself; listed hosts, by name before any DNS.
//! And where local DNS works, a name that does not exist never opens the path.

use std::net::SocketAddr;

use clauseo_blocklist::Tag;

use super::blocked::{blocked, current};
use super::harness::{Setup, host_denial, rig, upstream};
use super::open_probe::probing;
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
fn nonexistent_names_never_open_the_path_where_local_dns_works() {
    let (address, heads) = upstream(OPEN);
    let rig = probing(address, true, None);
    rig.net.name("inside.test", &["10.0.0.9"]);
    let code = |host: &str| rig.browser_connect(&format!("{host}:443")).1;
    for host in [
        "x1.nonexistent.test",
        "x2.nonexistent.test",
        "x3.nonexistent.test",
    ] {
        assert_eq!(code(host), 403, "{host}");
    }
    assert_eq!(code("inside.test"), 403, "still looked up and checked");
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("x1.nonexistent.test", 443, DnsUnavailable, false),
            host_denial("x2.nonexistent.test", 443, DnsUnavailable, false),
            host_denial("x3.nonexistent.test", 443, DnsUnavailable, false),
            host_denial("inside.test", 443, PrivateAddress, false),
        ]
    );
    assert_eq!(rig.net.resolved().len(), 4, "every name looked up");
    assert_eq!(heads.try_iter().count(), 0, "nothing upstream");
}

#[test]
fn the_firm_proxy_s_own_host_is_refused_by_name() {
    let url = url::Url::parse("http://proxy.firm.test:3128").unwrap();
    let rig = rig(Setup {
        mode: ProxyMode::Open,
        upstream: Upstream::Fixed(url),
        ..Setup::default()
    });
    for target in ["proxy.firm.test:443", "Proxy.Firm.Test.:443"] {
        assert_eq!(rig.browser_connect(target).1, 403, "{target}");
    }
    let own = host_denial("proxy.firm.test", 443, DnsUnavailable, false);
    assert_eq!(rig.denials(), vec![own.clone(), own]);
    assert_eq!(
        rig.net.dialled(),
        Vec::<SocketAddr>::new(),
        "the proxy never reached"
    );
}

#[test]
fn names_that_spell_an_address_are_refused_by_name() {
    let (address, heads) = upstream(OPEN);
    let rig = through(address, ProxyMode::Open, &[], &[]);
    let refused = [
        "10-0-0-5.sslip.io",
        "127.0.0.1.nip.io",
        "app.localtest.me",
        "x.lvh.me",
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
}

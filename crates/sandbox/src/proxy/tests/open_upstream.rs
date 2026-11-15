//! Open mode through the firm's proxy: a name let in without the list (the browser listener)
//! goes upstream only once local DNS has checked it. A local lookup that fails, times out or
//! finds nothing refuses it as `DnsUnavailable` (a private answer as `PrivateAddress`), with
//! nothing sent upstream; a name the list vouched for (allowlist mode, or the agent listener in
//! open mode) still goes on unchecked.

use std::net::SocketAddr;
use std::time::Duration;

use super::harness::{Rig, Setup, host_denial, rig, upstream};
use crate::HostRefusal::{DnsUnavailable, PrivateAddress};
use crate::Upstream;
use crate::proxy::{Limits, ProxyMode};

const OPEN: &str = "HTTP/1.1 200 Connection established\r\n\r\n";

/// A rig in `mode` allowing `allow`, through the upstream at `address`, with a short connect
/// limit so a held lookup times out quickly.
fn through(address: SocketAddr, mode: ProxyMode, allow: &[&'static str]) -> Rig {
    let url = url::Url::parse(&format!("http://{address}")).unwrap();
    rig(Setup {
        mode,
        allow: allow.to_vec(),
        upstream: Upstream::Fixed(url),
        limits: Limits {
            connect: Duration::from_millis(200),
            ..Limits::default()
        },
        ..Setup::default()
    })
}

#[test]
fn open_mode_through_an_upstream_refuses_a_name_local_dns_cannot_check() {
    let (address, heads) = upstream(OPEN);
    let rig = through(address, ProxyMode::Open, &[]);
    rig.net.name("empty.test", &[]);
    // No local answer for `unknown.test`: the lookup fails.
    assert_eq!(rig.browser_connect("unknown.test:443").1, 403);
    let answer =
        rig.browser_exchange("GET http://unknown.test/ HTTP/1.1\r\nHost: unknown.test\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert_eq!(rig.browser_connect("empty.test:443").1, 403);
    rig.net.name("inside.test", &["10.0.0.9"]);
    assert_eq!(rig.browser_connect("inside.test:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("unknown.test", 443, DnsUnavailable, false),
            host_denial("unknown.test", 80, DnsUnavailable, false),
            host_denial("empty.test", 443, DnsUnavailable, false),
            host_denial("inside.test", 443, PrivateAddress, false),
        ]
    );
    assert_eq!(
        rig.net.dialled(),
        Vec::<SocketAddr>::new(),
        "nothing upstream"
    );
    assert_eq!(heads.try_iter().count(), 0);

    // A name whose local answers are all public still goes on.
    rig.net.name("public.test", &["192.0.2.20"]);
    assert_eq!(rig.browser_connect("public.test:443").1, 200);
    assert!(
        heads
            .recv()
            .unwrap()
            .starts_with("CONNECT public.test:443 ")
    );
    assert_eq!(rig.net.dialled(), vec![address]);
}

#[test]
fn open_mode_through_an_upstream_refuses_a_name_whose_local_lookup_times_out() {
    let (address, heads) = upstream(OPEN);
    let rig = through(address, ProxyMode::Open, &[]);
    rig.net.name("slow.test", &["192.0.2.20"]);
    // Never released: each local lookup times out after the connect limit.
    let _first = rig.net.hold_next_resolve();
    assert_eq!(rig.browser_connect("slow.test:443").1, 403);
    let _second = rig.net.hold_next_resolve();
    let answer = rig.browser_exchange("GET http://slow.test/ HTTP/1.1\r\nHost: slow.test\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("slow.test", 443, DnsUnavailable, false),
            host_denial("slow.test", 80, DnsUnavailable, false),
        ]
    );
    assert_eq!(
        rig.net.dialled(),
        Vec::<SocketAddr>::new(),
        "nothing upstream"
    );
    assert_eq!(heads.try_iter().count(), 0);
}

/// The known gap stays where the list vouched for the name: allowlist mode on either listener,
/// and the agent listener in open mode.
#[test]
fn a_listed_name_local_dns_cannot_find_still_goes_upstream() {
    for (mode, browser) in [
        (ProxyMode::Allowlist, false),
        (ProxyMode::Allowlist, true),
        (ProxyMode::Open, false),
    ] {
        let (address, heads) = upstream(OPEN);
        let rig = through(address, mode, &["unknown.test"]);
        let code = if browser {
            rig.browser_connect("unknown.test:443").1
        } else {
            rig.connect("unknown.test:443").1
        };
        assert_eq!(code, 200, "{mode:?} browser {browser}");
        let head = heads.recv().unwrap();
        assert!(head.starts_with("CONNECT unknown.test:443 "), "{head}");
        assert_eq!(rig.denials(), Vec::new(), "{mode:?} browser {browser}");
    }
}

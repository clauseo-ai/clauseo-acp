//! Open mode through the firm's proxy (`name_only`): a name let in without the list (the browser
//! listener) is checked by local DNS when it answers, a private answer refused as before; when
//! local DNS fails, times out or finds nothing on a network whose DNS answers no public name (the
//! probe's names unknown to the test network), a CONNECT goes to the firm's proxy by name, and a
//! plain `http://` request is refused (`DnsUnavailable`). A direct route still needs local DNS,
//! and a name the list vouched for (allowlist mode, or the agent listener in open mode) still
//! goes on unchecked.

use std::net::SocketAddr;
use std::time::Duration;

use super::harness::{Rig, Setup, host_denial, rig, upstream};
use crate::HostRefusal::{DnsUnavailable, PrivateAddress};
use crate::Upstream;
use crate::proxy::{Limits, ProxyMode};

pub(super) const OPEN: &str = "HTTP/1.1 200 Connection established\r\n\r\n";

/// A rig in `mode` allowing `allow`, through the upstream at `address`, on a network whose DNS
/// answers no probe name, with the firm's own suffixes `firm` and a short connect limit (under
/// the name-only path's 2 s), so a held lookup times out quickly.
pub(super) fn through(
    address: SocketAddr,
    mode: ProxyMode,
    allow: &[&'static str],
    firm: &[&'static str],
) -> Rig {
    let url = url::Url::parse(&format!("http://{address}")).unwrap();
    rig(Setup {
        mode,
        allow: allow.to_vec(),
        upstream: Upstream::Fixed(url),
        limits: Limits {
            connect: Duration::from_millis(200),
            ..Limits::default()
        },
        firm: firm.to_vec(),
        ..Setup::default()
    })
}

#[test]
fn with_local_dns_failing_a_public_name_goes_to_the_firm_s_proxy_by_name() {
    let (address, heads) = upstream(OPEN);
    let rig = through(address, ProxyMode::Open, &[], &[]);
    // No local answer for `unknown.test`: the lookup fails. `empty.test` answers nothing.
    rig.net.name("empty.test", &[]);
    for host in ["unknown.test", "empty.test"] {
        assert_eq!(rig.browser_connect(&format!("{host}:443")).1, 200, "{host}");
        let head = heads.recv().unwrap();
        assert!(head.starts_with(&format!("CONNECT {host}:443 ")), "{head}");
    }
    // A lookup that times out (never released) too.
    rig.net.name("slow.test", &["192.0.2.20"]);
    let _held = rig.net.hold_next_resolve();
    assert_eq!(rig.browser_connect("slow.test:443").1, 200);
    let head = heads.recv().unwrap();
    assert!(head.starts_with("CONNECT slow.test:443 "), "{head}");
    assert_eq!(rig.denials(), Vec::new());
    assert_eq!(
        rig.net.resolved(),
        ["unknown.test", "empty.test", "slow.test"]
    );
    assert_eq!(rig.net.dialled(), vec![address; 3], "only the firm's proxy");
}

#[test]
fn a_local_answer_is_still_checked_and_a_private_one_refused() {
    let (address, heads) = upstream(OPEN);
    let rig = through(address, ProxyMode::Open, &[], &[]);
    rig.net.name("inside.test", &["10.0.0.9"]);
    rig.net.name("mixed.test", &["192.0.2.20", "127.0.0.1"]);
    assert_eq!(rig.browser_connect("inside.test:443").1, 403);
    assert_eq!(rig.browser_connect("mixed.test:443").1, 403);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("inside.test", 443, PrivateAddress, false),
            host_denial("mixed.test", 443, PrivateAddress, false),
        ]
    );
    assert_eq!(heads.try_iter().count(), 0, "nothing upstream");
    // A name whose local answers are all public goes on.
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
fn plain_http_on_the_name_only_path_is_refused() {
    let (address, heads) = upstream(OPEN);
    let rig = through(address, ProxyMode::Open, &[], &[]);
    let answer =
        rig.browser_exchange("GET http://unknown.test/ HTTP/1.1\r\nHost: unknown.test\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert_eq!(
        rig.denials(),
        vec![host_denial("unknown.test", 80, DnsUnavailable, false)]
    );
    assert_eq!(heads.try_iter().count(), 0, "nothing upstream");
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

#[test]
fn a_direct_route_with_local_dns_failing_still_gets_502() {
    let rig = rig(Setup {
        mode: ProxyMode::Open,
        ..Setup::default()
    });
    assert_eq!(rig.browser_connect("unknown.test:443").1, 502);
    let answer =
        rig.browser_exchange("GET http://unknown.test/ HTTP/1.1\r\nHost: unknown.test\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
    assert_eq!(rig.denials(), Vec::new());
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
}

/// The known gap stays where the list vouched for the name: allowlist mode on either listener,
/// and the agent listener in open mode, where no name check applies either.
#[test]
fn a_listed_name_local_dns_cannot_find_still_goes_upstream() {
    for (mode, browser) in [
        (ProxyMode::Allowlist, false),
        (ProxyMode::Allowlist, true),
        (ProxyMode::Open, false),
    ] {
        let (address, heads) = upstream(OPEN);
        let allow = ["unknown.test", "wiki.corp", "dms.firm.example"];
        let rig = through(address, mode, &allow, &["firm.example"]);
        for host in allow {
            let target = format!("{host}:443");
            let code = if browser {
                rig.browser_connect(&target).1
            } else {
                rig.connect(&target).1
            };
            assert_eq!(code, 200, "{mode:?} browser {browser} {host}");
            let head = heads.recv().unwrap();
            assert!(head.starts_with(&format!("CONNECT {target} ")), "{head}");
        }
        assert_eq!(rig.denials(), Vec::new(), "{mode:?} browser {browser}");
    }
}

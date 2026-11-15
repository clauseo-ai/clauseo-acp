//! The machine's own networks (`local_net`): an interface address and every address on its
//! on-link prefix are refused as `PrivateAddress`, though public, by name and by IP literal, on
//! a direct route and through an upstream, in open mode and allowlist mode alike. An address
//! off those networks passes.

use std::net::SocketAddr;

use super::harness::{Rig, Setup, echo, host_denial, rig, upstream};
use crate::HostRefusal::PrivateAddress;
use crate::Upstream;
use crate::proxy::ProxyMode;

/// The machine's networks: a global IPv6 /64 and a public IPv4 /24 (documentation ranges,
/// which the public-address check passes).
const LOCAL: &[(&str, u8)] = &[("2001:db8:1:2::10", 64), ("203.0.113.7", 24)];

/// Names on and off those networks, each with an echo server on 443.
fn names(rig: &Rig) {
    let tls = echo();
    for (name, ip) in [
        ("own6.test", "2001:db8:1:2::10"),
        ("router6.test", "2001:db8:1:2::1"),
        ("own4.test", "203.0.113.7"),
        ("router4.test", "203.0.113.1"),
        ("mapped.test", "::ffff:203.0.113.1"),
        ("outside6.test", "2001:db8:1:3::1"),
        ("outside4.test", "198.51.100.7"),
    ] {
        rig.net.name(name, &[ip]);
        let ip: std::net::IpAddr = ip.parse().unwrap();
        rig.net.route(&SocketAddr::new(ip, 443).to_string(), tls);
    }
}

const ON_LINK: &[&str] = &[
    "own6.test",
    "router6.test",
    "own4.test",
    "router4.test",
    "mapped.test",
];

#[test]
fn open_mode_refuses_the_machine_s_networks_by_name_and_literal() {
    let rig = rig(Setup {
        mode: ProxyMode::Open,
        local: LOCAL.to_vec(),
        ..Setup::default()
    });
    names(&rig);
    for name in ON_LINK {
        let (_stream, code) = rig.browser_connect(&format!("{name}:443"));
        assert_eq!(code, 403, "{name}");
    }
    for literal in ["[2001:db8:1:2::1]:443", "203.0.113.1:443"] {
        assert_eq!(rig.browser_connect(literal).1, 403, "{literal}");
    }
    for name in ["outside6.test", "outside4.test"] {
        assert_eq!(rig.browser_connect(&format!("{name}:443")).1, 200, "{name}");
    }
    let denials = rig.denials();
    for name in ON_LINK {
        assert!(
            denials.contains(&host_denial(name, 443, PrivateAddress, false)),
            "{name}: {denials:?}"
        );
    }
    assert_eq!(denials.len(), ON_LINK.len() + 2, "{denials:?}");
}

#[test]
fn allowlist_mode_refuses_an_allowed_name_on_the_machine_s_networks() {
    let rig = rig(Setup {
        allow: vec!["router6.test", "outside6.test"],
        local: LOCAL.to_vec(),
        ..Setup::default()
    });
    names(&rig);
    assert_eq!(rig.connect("router6.test:443").1, 403);
    assert_eq!(rig.connect("outside6.test:443").1, 200);
    assert_eq!(
        rig.denials(),
        vec![host_denial("router6.test", 443, PrivateAddress, false)]
    );
}

#[test]
fn through_an_upstream_a_name_on_the_machine_s_networks_is_refused() {
    let (address, heads) = upstream("HTTP/1.1 200 Connection established\r\n\r\n");
    let url = url::Url::parse(&format!("http://{address}")).unwrap();
    let rig = rig(Setup {
        mode: ProxyMode::Open,
        upstream: Upstream::Fixed(url),
        local: LOCAL.to_vec(),
        ..Setup::default()
    });
    names(&rig);
    assert_eq!(rig.browser_connect("router6.test:443").1, 403);
    assert_eq!(rig.browser_connect("203.0.113.1:443").1, 403);
    assert_eq!(heads.try_iter().count(), 0, "nothing went upstream");
    assert_eq!(rig.browser_connect("outside6.test:443").1, 200);
    assert_eq!(
        rig.denials()[0],
        host_denial("router6.test", 443, PrivateAddress, false)
    );
}

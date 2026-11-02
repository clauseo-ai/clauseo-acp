//! The firm's proxy: CONNECT relayed upstream, its 407, plain `http://` absolute-form, and the
//! address check through it.

use std::net::SocketAddr;

use super::harness::{Rig, Setup, host_denial, origin, rig, round_trip, upstream};
use crate::HostRefusal::PrivateAddress;
use crate::proxy::Verdict;
use crate::{Denial, Error, ProxyConfig, Upstream, start_proxy};

fn through(address: SocketAddr, allow: &[&'static str]) -> Rig {
    let url = url::Url::parse(&format!("http://{address}")).unwrap();
    rig(Setup {
        allow: allow.to_vec(),
        upstream: Upstream::Fixed(url),
        ..Setup::default()
    })
}

#[test]
fn a_connect_goes_to_the_upstream_unresolved_and_its_200_is_relayed() {
    let (address, heads) = upstream("HTTP/1.1 200 Connection established\r\n\r\n");
    let rig = through(address, &["api.anthropic.com"]);
    let (mut tunnel, code) = rig.connect("api.anthropic.com:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"tls bytes"), b"tls bytes");
    assert_eq!(
        heads.recv().unwrap(),
        "CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\n\r\n",
        "no credentials go upstream"
    );
    assert!(
        rig.net.resolved().is_empty(),
        "the upstream resolves the name"
    );
    assert_eq!(rig.net.dialled(), vec![address]);
}

#[test]
fn an_upstream_407_gives_502_and_an_upstream_auth_denial() {
    let (address, _heads) =
        upstream("HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: NTLM\r\n\r\n");
    let rig = through(address, &["api.anthropic.com"]);
    assert_eq!(rig.connect("api.anthropic.com:443").1, 502);
    assert_eq!(
        rig.denials(),
        vec![Denial::UpstreamAuth {
            upstream: address.to_string()
        }]
    );
    assert_eq!(rig.records()[0].verdict, Verdict::UpstreamAuth);
}

#[test]
fn any_other_upstream_answer_gives_502_and_no_denial() {
    let (address, _heads) = upstream("HTTP/1.1 503 Service Unavailable\r\n\r\n");
    let rig = through(address, &["api.anthropic.com"]);
    assert_eq!(rig.connect("api.anthropic.com:443").1, 502);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

#[test]
fn plain_http_goes_to_the_upstream_in_absolute_form() {
    let (address, received) = origin("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    let rig = through(address, &["allowed.test"]);
    let answer = rig.exchange("GET http://allowed.test/x?y HTTP/1.1\r\nAccept: */*\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert_eq!(
        String::from_utf8(received.recv().unwrap()).unwrap(),
        "GET http://allowed.test/x?y HTTP/1.1\r\nHost: allowed.test\r\nAccept: */*\r\n\
         Connection: close\r\n\r\n"
    );
    assert_eq!(rig.net.resolved(), Vec::<String>::new());
}

#[test]
fn through_an_upstream_only_ip_literals_are_address_checked() {
    let (address, _heads) = upstream("HTTP/1.1 200 Connection established\r\n\r\n");
    let rig = through(
        address,
        &["10.0.0.8", "[fd00::8]", "192.0.2.10", "intranet.test"],
    );
    assert_eq!(rig.connect("10.0.0.8:443").1, 403);
    assert_eq!(rig.connect("[fd00::8]:443").1, 403);
    assert_eq!(rig.connect("192.0.2.10:443").1, 200);
    // A name goes to the upstream unresolved, whatever it would resolve to.
    rig.net.name("intranet.test", &["10.0.0.9"]);
    assert_eq!(rig.connect("intranet.test:443").1, 200);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("10.0.0.8", 443, PrivateAddress, false),
            host_denial("fd00::8", 443, PrivateAddress, false),
        ]
    );
    assert_eq!(rig.net.resolved(), Vec::<String>::new());
}

#[test]
fn a_fixed_upstream_must_be_a_bare_http_url() {
    for bad in [
        "https://proxy.test:8443",
        "http://user:pw@proxy.test:8080",
        "http://proxy.test:8080/path",
        "http://proxy.test:8080/?q",
        "socks5://proxy.test:1080",
    ] {
        let config = ProxyConfig {
            allow: crate::Allowlist::default(),
            silent: crate::Allowlist::default(),
            upstream: Upstream::Fixed(url::Url::parse(bad).unwrap()),
            denials: std::sync::Arc::new(super::harness::Sink::default()),
            record: None,
        };
        let error = start_proxy(config).unwrap_err();
        assert!(matches!(error, Error::ProxyConfig(_)), "{bad}: {error}");
        assert!(!format!("{error}").contains("pw"), "{error}");
    }
}

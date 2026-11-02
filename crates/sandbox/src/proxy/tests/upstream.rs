//! The firm's proxy: CONNECT relayed upstream, its 407, plain `http://` absolute-form, the
//! address check through it, and the host's proxy variables.

use std::net::SocketAddr;
use std::time::Duration;

use super::harness::{Rig, Setup, host_denial, origin, rig, round_trip, upstream};
use crate::HostRefusal::PrivateAddress;
use crate::proxy::upstream::Env;
use crate::proxy::{Limits, Verdict};
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
        "no credentials go upstream, and the name goes unresolved"
    );
    // Resolved locally only for the address check (the test net has no answer for it).
    assert_eq!(rig.net.resolved(), vec!["api.anthropic.com".to_owned()]);
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
    assert_eq!(rig.net.dialled(), vec![address]);
}

#[test]
fn a_plain_http_407_from_the_upstream_gives_502_and_an_upstream_auth_denial() {
    let (address, _received) = origin(
        "HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: NTLM\r\n\
         Content-Length: 0\r\n\r\n",
    );
    let rig = through(address, &["allowed.test"]);
    let answer = rig.exchange("GET http://allowed.test/ HTTP/1.1\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 502 "), "{answer}");
    assert!(!answer.contains("NTLM"), "{answer}");
    assert_eq!(
        rig.denials(),
        vec![Denial::UpstreamAuth {
            upstream: address.to_string()
        }]
    );
    assert_eq!(rig.records()[0].verdict, Verdict::UpstreamAuth);
}

#[test]
fn through_an_upstream_names_are_resolved_locally_too_and_private_answers_refused() {
    let (address, heads) = upstream("HTTP/1.1 200 Connection established\r\n\r\n");
    let rig = through(
        address,
        &[
            "10.0.0.8",
            "[fd00::8]",
            "192.0.2.10",
            "intranet.test",
            "localhost",
            "mixed.test",
            "public.test",
            "unknown.test",
        ],
    );
    rig.net.name("intranet.test", &["10.0.0.9"]);
    rig.net.name("localhost", &["127.0.0.1", "::1"]);
    rig.net.name("mixed.test", &["192.0.2.30", "192.168.1.1"]);
    rig.net.name("public.test", &["192.0.2.20"]);
    assert_eq!(rig.connect("10.0.0.8:443").1, 403);
    assert_eq!(rig.connect("[fd00::8]:443").1, 403);
    assert_eq!(rig.connect("intranet.test:443").1, 403);
    assert_eq!(rig.connect("localhost:443").1, 403);
    assert_eq!(rig.connect("mixed.test:443").1, 403);
    assert_eq!(rig.connect("192.0.2.10:443").1, 200);
    assert_eq!(rig.connect("public.test:443").1, 200);
    // No local answer: the desktop may resolve only through the proxy, so the name goes on.
    assert_eq!(rig.connect("unknown.test:443").1, 200);
    assert_eq!(
        rig.denials(),
        vec![
            host_denial("10.0.0.8", 443, PrivateAddress, false),
            host_denial("fd00::8", 443, PrivateAddress, false),
            host_denial("intranet.test", 443, PrivateAddress, false),
            host_denial("localhost", 443, PrivateAddress, false),
            host_denial("mixed.test", 443, PrivateAddress, false),
        ]
    );
    let sent: Vec<String> = heads.try_iter().collect();
    assert_eq!(sent.len(), 3);
    assert!(sent[1].starts_with("CONNECT public.test:443 "), "{sent:?}");
    assert!(sent[2].starts_with("CONNECT unknown.test:443 "), "{sent:?}");
    // Only the upstream is ever dialled, never a resolved address.
    assert!(rig.net.dialled().iter().all(|a| *a == address));
}

#[test]
fn through_an_upstream_a_local_lookup_that_times_out_lets_the_name_go_on() {
    let (address, heads) = upstream("HTTP/1.1 200 Connection established\r\n\r\n");
    let url = url::Url::parse(&format!("http://{address}")).unwrap();
    let rig = rig(Setup {
        allow: vec!["slow.test"],
        upstream: Upstream::Fixed(url),
        limits: Limits {
            connect: Duration::from_millis(200),
            ..Limits::default()
        },
        ..Setup::default()
    });
    rig.net.name("slow.test", &["10.0.0.9"]);
    // Never released: the local lookup times out after the connect limit.
    let _held = rig.net.hold_next_resolve();
    assert_eq!(rig.connect("slow.test:443").1, 200);
    assert!(heads.recv().unwrap().starts_with("CONNECT slow.test:443 "));
}

#[test]
fn https_proxy_from_the_host_env_carries_a_connect_end_to_end() {
    let (address, heads) = upstream("HTTP/1.1 200 Connection established\r\n\r\n");
    let rig = rig(Setup {
        allow: vec!["api.anthropic.com"],
        upstream: Upstream::System,
        env: Some(Env {
            https: Some(format!("http://user:secret@{address}")),
            ..Env::default()
        }),
        ..Setup::default()
    });
    let (mut tunnel, code) = rig.connect("api.anthropic.com:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"tls"), b"tls");
    assert_eq!(
        heads.recv().unwrap(),
        "CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\n\r\n"
    );
    assert_eq!(rig.net.dialled(), vec![address]);
}

#[test]
fn http_proxy_from_the_host_env_carries_plain_http_and_https_proxy_does_not() {
    let (plain, received) = origin("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    let (secure, _heads) = upstream("HTTP/1.1 200 Connection established\r\n\r\n");
    let rig = rig(Setup {
        allow: vec!["allowed.test"],
        upstream: Upstream::System,
        env: Some(Env {
            https: Some(format!("http://{secure}")),
            http: Some(format!("http://{plain}")),
            no_proxy: None,
        }),
        ..Setup::default()
    });
    let answer = rig.exchange("GET http://allowed.test/ HTTP/1.1\r\n\r\n");
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert!(
        String::from_utf8(received.recv().unwrap())
            .unwrap()
            .starts_with("GET http://allowed.test/ HTTP/1.1\r\n")
    );
    assert_eq!(rig.net.dialled(), vec![plain]);
}

#[test]
fn debug_of_a_config_never_shows_the_upstream_password_or_path() {
    let config = ProxyConfig {
        allow: crate::Allowlist::default(),
        silent: crate::Allowlist::default(),
        upstream: Upstream::Fixed(
            url::Url::parse("http://user:pw-secret@proxy.test:8080/p?q").unwrap(),
        ),
        denials: std::sync::Arc::new(super::harness::Sink::default()),
        record: None,
    };
    let debug = format!("{config:?}");
    assert!(debug.contains("Fixed(http://proxy.test:8080)"), "{debug}");
    assert!(
        !debug.contains("pw-secret") && !debug.contains("user") && !debug.contains("/p?q"),
        "{debug}"
    );
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

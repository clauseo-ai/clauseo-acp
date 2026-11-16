use serde_json::json;

use super::super::tests::{claude, of};
use clauseo_protocol::AlwaysKind;

use super::super::well_formed;

/// Claude's shell asking for a host keeps the same site entry as a `WebFetch` of it.
#[test]
fn a_sandbox_network_host_is_the_webfetch_site() {
    let cases = [
        ("b.com", Some("WebFetch(domain:b.com)")),
        ("B.COM", Some("WebFetch(domain:b.com)")),
        ("bücher.de", Some("WebFetch(domain:xn--bcher-kva.de)")),
        ("8.8.8.8", Some("WebFetch(domain:8.8.8.8)")),
        ("10.0.0.1", None),
        ("a.com:443", None),
        ("u@b.com", None),
        ("b.com/x", None),
        ("x.com.", None),
        ("b.com?x", None),
        ("b.com#x", None),
        (r"b\c.com", None),
        ("b .com", None),
        ("", None),
        ("[::1]", None),
    ];
    for (host, want) in cases {
        let got = of("SandboxNetworkAccess", &json!({ "host": host }));
        assert_eq!(got.as_deref(), want, "{host:?}");
        if let Some(rule) = &got {
            assert!(well_formed(rule), "{rule}");
            assert_eq!(
                of("WebFetch", &json!({"url": format!("https://{host}/a")})),
                got
            );
        }
    }
    let kept = claude(
        "SandboxNetworkAccess",
        Some(&json!({"host": "b.com"})),
        false,
    )
    .unwrap();
    assert_eq!(kept.offer.kind, AlwaysKind::Site);
    assert_eq!(kept.offer.shown.as_deref(), Some("b.com"));
    assert_eq!(of("SandboxNetworkAccess", &json!({ "host": 1 })), None);
    // A trailing dot is no second entry for a WebFetch either.
    assert_eq!(of("WebFetch", &json!({"url": "https://x.com./"})), None);
    assert!(!well_formed("WebFetch(domain:x.com.)"));
}

#[test]
fn kept_sites_are_the_webfetch_hosts_in_order() {
    let rules = [
        "Bash(ls)",
        "WebFetch(domain:b.com)",
        "WebFetch(domain:x.com.)",
        "WebFetch(domain:*.gov.in)",
        "mcp__s__t",
        "WebFetch(domain:a.in)",
    ]
    .map(str::to_owned);
    assert_eq!(super::kept_sites(&rules), ["b.com", "a.in"]);
    assert_eq!(super::kept_sites(&[]), Vec::<String>::new());
}

/// A kept site is a site on the internet: never this machine or the lawyer's own network, which
/// a site rule would open to every later fetch and, shared, to Claude's shell.
#[test]
fn only_a_public_site_is_kept() {
    for host in [
        "localhost",
        "a.localhost",
        "intranet",
        "printer.local",
        "nas.internal",
        "router.lan",
        "box.home.arpa",
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "255.255.255.255",
        "1.2.3.999",
    ] {
        assert_eq!(
            of("SandboxNetworkAccess", &json!({ "host": host })),
            None,
            "{host}"
        );
        let url = format!("http://{host}/x");
        assert_eq!(of("WebFetch", &json!({ "url": url })), None, "{host}");
        assert!(!well_formed(&format!("WebFetch(domain:{host})")), "{host}");
    }
    for host in ["sci.gov.in", "8.8.8.8", "100.128.0.1", "indiankanoon.org"] {
        assert!(
            of("SandboxNetworkAccess", &json!({ "host": host })).is_some(),
            "{host}"
        );
    }
}

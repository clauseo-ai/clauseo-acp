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
        ("10.0.0.1", Some("WebFetch(domain:10.0.0.1)")),
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

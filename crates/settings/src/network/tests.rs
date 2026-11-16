//! The `network` keys' typing: what each accepts, what refuses the key, what leaves one entry
//! out, and that no warning quotes a value.

use super::{allowed_domains, ca_certs, domain_ok, upstream_proxy};
use crate::testing::{abs, abs_str};
use crate::warning::WarningKind;
use serde_json::json;
use std::collections::BTreeMap;

/// The keys of `dropped`'s warnings, in order.
fn keys(dropped: &[WarningKind]) -> Vec<&str> {
    dropped
        .iter()
        .map(|kind| match kind {
            WarningKind::InvalidValue { key, .. } => key.as_str(),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn upstream_proxy_takes_http_or_https_with_a_host_or_null() {
    let key = "network.upstream_proxy";
    for url in [
        "http://proxy.firm.example:8080",
        "HTTP://proxy.firm.example",
        "https://proxy.firm.example/",
        "http://[fd00::1]:3128",
        "http://10.0.0.5:8080",
    ] {
        assert_eq!(upstream_proxy(key, json!(url)), Ok(Some(url.to_owned())));
    }
    assert_eq!(upstream_proxy(key, json!(null)), Ok(None));
    for bad in [
        json!("socks5://proxy.firm.example:1080"),
        json!("proxy.firm.example:8080"),
        json!("http://user:secret@proxy.firm.example"),
        json!("http://proxy.firm.example:99999"),
        json!("http://"),
        json!("http://proxy.firm.example/?pac=1"),
        json!("http://pröxy.firm.example"),
        json!(8080),
        json!(true),
    ] {
        match upstream_proxy(key, bad.clone()) {
            Err(WarningKind::InvalidValue { key: k, message }) => {
                assert_eq!(k, key);
                assert!(!message.contains("secret"), "{message}");
            }
            other => panic!("{bad}: {other:?}"),
        }
    }
}

#[test]
fn ca_certs_keeps_good_entries_and_drops_bad_ones_by_id_or_place() {
    let key = "network.ca_certs";
    let mut dropped = Vec::new();
    let value = json!({
        "claude-acp": abs_str("ca/claude.pem"),
        "codex-acp": "relative/ca.pem",
        "Bad Id with secret": abs_str("ca/x.pem"),
        "fake-agent": 7,
    });
    let certs = ca_certs(key, value, &mut dropped).unwrap();
    assert_eq!(
        certs,
        BTreeMap::from([("claude-acp".to_owned(), abs("ca/claude.pem"))])
    );
    // A workspace build keeps the map's order and a one-crate build sorts it, so the bad id's
    // place is either; the other two are named by id.
    let mut named = keys(&dropped);
    named.sort_unstable();
    assert_eq!(named.len(), 3);
    assert!(
        named[0].starts_with("network.ca_certs.<entry "),
        "{named:?}"
    );
    assert_eq!(
        named[1..],
        ["network.ca_certs.codex-acp", "network.ca_certs.fake-agent"]
    );
    assert!(!format!("{dropped:?}").contains("secret"));
    for bad in [json!([]), json!("x"), json!(null)] {
        let mut dropped = Vec::new();
        assert!(ca_certs(key, bad, &mut dropped).is_err());
        assert_eq!(dropped, Vec::new());
    }
}

#[test]
fn allowed_domains_lowercases_keeps_each_once_and_drops_bad_entries_by_place() {
    let key = "network.allowed_domains";
    let mut dropped = Vec::new();
    let value = json!([
        "Docs.Firm.example",
        "*.courts.example",
        "docs.firm.example",
        "https://secret.example",
        "secret.example:443",
        "10.0.0.5",
        "localhost",
        3,
    ]);
    let hosts = allowed_domains(key, value, &mut dropped).unwrap();
    assert_eq!(hosts, ["docs.firm.example", "*.courts.example"]);
    assert_eq!(
        keys(&dropped),
        [
            "network.allowed_domains.<entry 4>",
            "network.allowed_domains.<entry 5>",
            "network.allowed_domains.<entry 6>",
            "network.allowed_domains.<entry 7>",
            "network.allowed_domains.<entry 8>",
        ]
    );
    assert!(!format!("{dropped:?}").contains("secret"));
    let mut dropped = Vec::new();
    assert!(allowed_domains(key, json!("docs.firm.example"), &mut dropped).is_err());
    assert_eq!(
        allowed_domains(key, json!([]), &mut dropped),
        Ok(Vec::new())
    );
}

#[test]
fn a_domain_is_a_host_name_with_an_optional_wildcard() {
    for good in [
        "firm.example",
        "a.b.c.firm.example",
        "*.firm.example",
        "xn--prxy-1ra.example",
        "a-b.example",
        "1password.com",
    ] {
        assert!(domain_ok(good), "{good}");
    }
    let long_label = format!("{}.example", "a".repeat(64));
    let long_name = format!("{}example", "abcdefghi.".repeat(25));
    for bad in [
        "",
        "example",
        "*.example",
        "*",
        "*.*.firm.example",
        "firm.*.example",
        ".firm.example",
        "firm.example.",
        "firm..example",
        "-firm.example",
        "firm-.example",
        "fi rm.example",
        "fírm.example",
        "firm.example/path",
        "192.168.0.1",
        long_label.as_str(),
        long_name.as_str(),
    ] {
        assert!(!domain_ok(bad), "{bad}");
    }
}

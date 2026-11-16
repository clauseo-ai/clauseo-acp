//! `sandbox_network`: the shell's allowlist is the seeded fetch sites, the needed hosts and, in
//! `auto`, the matter's kept sites, and nothing wider.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::sandbox::DENIED_DOMAINS;
use super::sandbox_network::{
    SHELL_NEEDED_HOSTS, Strict, allowed_domains, fed_kept_hosts, is_plain_host, launch_network,
    network,
};
use crate::browser::seed_sites::WEB_FETCH_SITES;

fn kept(hosts: &[&str]) -> Vec<String> {
    hosts.iter().map(|host| (*host).to_owned()).collect()
}

#[test]
fn the_list_is_the_seeded_fetch_sites_and_the_needed_hosts() {
    let listed: BTreeSet<&str> = allowed_domains(&[]).into_iter().collect();
    let expected: BTreeSet<&str> = WEB_FETCH_SITES
        .iter()
        .chain(SHELL_NEEDED_HOSTS)
        .copied()
        .collect();
    assert_eq!(listed, expected);
}

#[test]
fn the_list_is_sorted_and_each_host_once() {
    for kept_hosts in [&[][..], &["sci.gov.in", "example.org", "example.org"][..]] {
        let hosts = allowed_domains(kept_hosts);
        assert!(hosts.windows(2).all(|pair| pair[0] < pair[1]), "{hosts:?}");
    }
}

/// No wildcard of any kind: a bare `*` or a `*.x` would reach more than the seeded
/// `WebFetch(domain:x)` rules, which match the host exactly.
#[test]
fn no_entry_is_a_wildcard() {
    for host in allowed_domains(&[]) {
        assert!(!host.contains('*'), "{host}");
    }
}

#[test]
fn no_entry_is_loopback_or_a_denied_host() {
    for host in allowed_domains(&[]) {
        assert!(!DENIED_DOMAINS.contains(&host), "{host}");
        assert!(
            host != "localhost" && !host.ends_with(".localhost"),
            "{host}"
        );
        assert!(
            host.parse::<std::net::IpAddr>().is_err(),
            "{host} is an address"
        );
    }
}

/// Each listed host is a valid DNS host name, by a check written out here rather than
/// `is_plain_host`, so a fault in that function cannot pass the list.
#[test]
fn every_entry_is_a_valid_host_name() {
    for host in allowed_domains(&[]) {
        assert!(!host.is_empty() && host.len() <= 253, "{host}");
        let labels: Vec<&str> = host.split('.').collect();
        assert!(labels.len() >= 2, "{host}");
        for label in &labels {
            assert!(
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{host}: label {label:?}"
            );
        }
        assert!(
            labels
                .last()
                .is_some_and(|tld| !tld.bytes().all(|b| b.is_ascii_digit())),
            "{host}"
        );
        assert!(is_plain_host(host), "{host}");
    }
}

#[test]
fn a_kept_site_goes_in_only_as_a_plain_host() {
    for host in ["example.org", "files.court.example.in", "a-b.co"] {
        assert!(is_plain_host(host), "{host}");
    }
    for host in [
        "",
        "*",
        "*.example.org",
        "example",
        "Example.org",
        "example.org.",
        ".example.org",
        "example..org",
        "-a.org",
        "a-.org",
        "127.0.0.1",
        "10.0.0.1",
        "::1",
        "[::1]",
        "localhost",
        "app.localhost",
        "example.org:443",
        "https://example.org",
        "example.org/x",
        "user@example.org",
        "exa mple.org",
        "exämple.org",
    ] {
        assert!(!is_plain_host(host), "{host:?}");
    }
    let long = format!("{}.org", "a".repeat(64));
    assert!(!is_plain_host(&long));
}

/// `auto` feeds the plain kept sites; the ask modes feed none.
#[test]
fn kept_sites_are_fed_only_under_strict() {
    let kept_hosts = kept(&["example.org", "*.evil.org", "localhost", "files.example.in"]);
    assert_eq!(
        fed_kept_hosts(Strict::On, &kept_hosts),
        ["example.org", "files.example.in"]
    );
    assert!(fed_kept_hosts(Strict::Off, &kept_hosts).is_empty());
}

#[test]
fn strict_is_on_in_auto_and_off_in_forced_ask() {
    assert_eq!(Strict::at_launch(false), Strict::On);
    assert_eq!(Strict::at_launch(true), Strict::Off);
}

#[test]
fn the_network_object_lists_the_sites_and_denies_loopback() {
    assert_eq!(
        network(Strict::Off, &[]),
        json!({
            "allowedDomains": allowed_domains(&[]),
            "deniedDomains": DENIED_DOMAINS,
        })
    );
}

/// Strict adds `strictAllowlist` and changes nothing else.
#[test]
fn strict_adds_only_the_strict_allowlist_key() {
    let mut expected = network(Strict::Off, &["example.org"]);
    if let Some(object) = expected.as_object_mut() {
        object.insert("strictAllowlist".to_owned(), json!(true));
    }
    assert_eq!(network(Strict::On, &["example.org"]), expected);
}

fn with_sandbox() -> Map<String, Value> {
    let mut settings = Map::new();
    settings.insert(
        "sandbox".to_owned(),
        json!({"enabled": true, "network": {"allowedDomains": ["*"]}}),
    );
    settings
}

/// The launch in `auto`: strict, and the kept sites in the list.
#[test]
fn an_auto_launch_is_strict_and_lists_the_kept_sites() {
    let mut settings = with_sandbox();
    launch_network(&mut settings, false, &kept(&["example.org", "*"]));
    assert_eq!(settings["sandbox"]["enabled"], json!(true));
    assert_eq!(
        settings["sandbox"]["network"],
        network(Strict::On, &["example.org"])
    );
}

/// The launch in forced ask: no strict, no kept sites, the hook's `*` gone all the same.
#[test]
fn a_forced_ask_launch_asks_and_lists_no_kept_site() {
    let mut settings = with_sandbox();
    launch_network(&mut settings, true, &kept(&["example.org"]));
    assert_eq!(settings["sandbox"]["network"], network(Strict::Off, &[]));
}

/// No sandbox (Windows, or the sandbox off): no `sandbox` object is made.
#[test]
fn no_sandbox_gets_no_network_object() {
    let mut settings = Map::new();
    launch_network(&mut settings, false, &kept(&["example.org"]));
    assert!(settings.is_empty());
}

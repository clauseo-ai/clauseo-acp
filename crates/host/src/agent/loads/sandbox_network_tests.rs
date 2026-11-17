//! `sandbox_network`: the shell's allowlist is the seeded fetch sites and the needed hosts,
//! nothing wider, and strict in every mode but forced ask.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::sandbox::DENIED_DOMAINS;
use super::sandbox_network::{
    SHELL_NEEDED_HOSTS, Strict, allowed_domains, is_plain_host, launch_network, network,
    plain_hosts,
};
use super::{CLAUDE_FALLBACK_MODE, CLAUDE_MODE, claude_mode};
use crate::agent::forced_ask::ASK_MODE;
use crate::browser::seed_sites::WEB_FETCH_SITES;

fn extra(hosts: &[&str]) -> Vec<String> {
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
    for extra_hosts in [&[][..], &["sci.gov.in", "example.org", "example.org"][..]] {
        let hosts = allowed_domains(extra_hosts);
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
fn an_extra_host_goes_in_only_as_a_plain_host() {
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

/// Only the plain extra hosts go in.
#[test]
fn only_plain_extra_hosts_go_in() {
    let hosts = extra(&["example.org", "*.evil.org", "localhost", "files.example.in"]);
    assert_eq!(plain_hosts(&hosts), ["example.org", "files.example.in"]);
}

/// Strict in every mode but forced ask, each mode's launch checked through its network object.
/// Settings are fixed at launch: auto ([`CLAUDE_MODE`]) is the launch outside forced ask, and
/// the acceptEdits fallback ([`CLAUDE_FALLBACK_MODE`]) and a later switch to the default mode
/// run on that launch's settings. Forced ask launches in the default mode ([`ASK_MODE`]).
#[test]
fn strict_in_auto_accept_edits_and_the_default_but_not_forced_ask() {
    // (the mode the session runs in, the mode it launched in, strict expected)
    let cases = [
        (CLAUDE_MODE, claude_mode(false), true),
        (CLAUDE_FALLBACK_MODE, claude_mode(false), true),
        ("default", claude_mode(false), true),
        (ASK_MODE, claude_mode(true), false),
    ];
    for (mode, launched, strict) in cases {
        let forced_ask = launched == ASK_MODE;
        assert_eq!(forced_ask, !strict, "{mode} launched as {launched}");
        let mut settings = with_sandbox();
        launch_network(&mut settings, forced_ask, true, &[]);
        let network = &settings["sandbox"]["network"];
        assert_eq!(
            network.get("strictAllowlist") == Some(&json!(true)),
            strict,
            "{mode}: {network}"
        );
        assert_eq!(
            network["allowedDomains"],
            json!(allowed_domains(&[])),
            "{mode}"
        );
    }
    assert_eq!(claude_mode(false), CLAUDE_MODE);
}

/// With `tools.web_search` off the shell's list is empty in every mode: strict and empty
/// reaches no host; forced ask still asks per site.
#[test]
fn web_off_empties_the_shells_list() {
    for forced_ask in [false, true] {
        let mut settings = with_sandbox();
        launch_network(&mut settings, forced_ask, false, &extra(&["example.org"]));
        let network = &settings["sandbox"]["network"];
        assert_eq!(network["allowedDomains"], json!([]), "{forced_ask}");
        assert_eq!(
            network["deniedDomains"],
            json!(DENIED_DOMAINS),
            "{forced_ask}"
        );
        assert_eq!(
            network.get("strictAllowlist").is_some(),
            !forced_ask,
            "{forced_ask}"
        );
    }
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

/// A launch outside forced ask: strict, the plain extra hosts in the list.
#[test]
fn a_launch_outside_forced_ask_is_strict() {
    let mut settings = with_sandbox();
    launch_network(&mut settings, false, true, &extra(&["example.org", "*"]));
    assert_eq!(settings["sandbox"]["enabled"], json!(true));
    assert_eq!(
        settings["sandbox"]["network"],
        network(Strict::On, &["example.org"])
    );
}

/// A forced-ask launch: no strict, so an unlisted host is a card; the hook's `*` gone all the
/// same.
#[test]
fn a_forced_ask_launch_is_not_strict() {
    let mut settings = with_sandbox();
    launch_network(&mut settings, true, true, &[]);
    assert_eq!(settings["sandbox"]["network"], network(Strict::Off, &[]));
}

/// No sandbox (Windows, or the sandbox off): no `sandbox` object is made.
#[test]
fn no_sandbox_gets_no_network_object() {
    let mut settings = Map::new();
    launch_network(&mut settings, false, true, &extra(&["example.org"]));
    assert!(settings.is_empty());
}

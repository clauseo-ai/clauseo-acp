//! The `sandbox` and `network` keys (decision record 0051): each key's default, a policy value
//! winning over the lawyer's and locking it, and an unreadable policy source locking each at its
//! safe value. `network.allowed_domains` is the firm's alone.

use super::Effective;
use crate::error::Error;
use crate::policy::registry::{self, RegValue};
use crate::policy::{PolicyRead, PolicySource};
use crate::schema::{NetworkSettings, SandboxSettings};
use crate::testing::{FakeSource, MemFiles, abs, abs_json, locations, policy, user};
use crate::warning::{Origin, Warning, WarningKind};
use std::collections::BTreeMap;
use std::path::PathBuf;

const KEYS: [&str; 5] = [
    "sandbox.required",
    "sandbox.allow_unprotected_auto",
    "network.upstream_proxy",
    "network.ca_certs",
    "network.allowed_domains",
];

/// `settings.json` with each key away from its default (`allowed_domains`, which it may not
/// set, included).
fn lawyers() -> MemFiles {
    user(&format!(
        r#"{{"sandbox": {{"required": true, "allow_unprotected_auto": false}},
            "network": {{"upstream_proxy": "http://proxy.home.example:3128",
                "ca_certs": {{"claude-acp": "{}"}},
                "allowed_domains": ["docs.home.example"]}}}}"#,
        abs_json("home-ca.pem")
    ))
}

/// The lawyer's `settings.json` with `allow_unprotected_auto` on, so a lock at `false` shows.
fn lawyers_auto_on() -> MemFiles {
    user(&format!(
        r#"{{"sandbox": {{"required": true, "allow_unprotected_auto": true}},
            "network": {{"upstream_proxy": "http://proxy.home.example:3128",
                "ca_certs": {{"claude-acp": "{}"}}}}}}"#,
        abs_json("home-ca.pem")
    ))
}

fn unreadable() -> FakeSource<impl Fn() -> PolicyRead> {
    FakeSource {
        name: "broken",
        read: || {
            PolicyRead::Unreadable(Error::Parse {
                path: abs("policy.json"),
                line: 1,
                column: 1,
                message: "expected value".to_owned(),
            })
        },
    }
}

/// The CA map `{id: path}`.
fn certs(id: &str, path: &str) -> BTreeMap<String, PathBuf> {
    BTreeMap::from([(id.to_owned(), abs(path))])
}

#[test]
fn the_defaults_require_nothing_allow_auto_and_add_nothing() {
    let effective = Effective::load(&locations(), &MemFiles::default(), &[]);
    let settings = effective.settings();
    assert!(!settings.sandbox.required);
    assert!(settings.sandbox.allow_unprotected_auto);
    assert_eq!(settings.sandbox, SandboxSettings::default());
    assert_eq!(settings.network.upstream_proxy, None);
    assert!(settings.network.ca_certs.is_empty());
    assert_eq!(settings.network.allowed_domains, Vec::<String>::new());
    assert_eq!(settings.network, NetworkSettings::default());
    assert_eq!(effective.locked().count(), 0);
}

/// The lawyer's own values load, unlocked, but for `allowed_domains`, which only a policy sets:
/// ignored with a warning, it adds no domain.
#[test]
fn the_lawyers_values_load_but_allowed_domains_is_ignored() {
    let effective = Effective::load(&locations(), &lawyers(), &[]);
    let settings = effective.settings();
    assert!(settings.sandbox.required);
    assert!(!settings.sandbox.allow_unprotected_auto);
    assert_eq!(
        settings.network.upstream_proxy.as_deref(),
        Some("http://proxy.home.example:3128")
    );
    assert_eq!(
        settings.network.ca_certs,
        certs("claude-acp", "home-ca.pem")
    );
    assert_eq!(settings.network.allowed_domains, Vec::<String>::new());
    assert_eq!(effective.locked().count(), 0);
    match effective.warnings() {
        [
            Warning {
                origin: Origin::File { .. },
                kind: WarningKind::InvalidValue { key, message },
            },
        ] => {
            assert_eq!(key, "network.allowed_domains");
            assert!(message.starts_with("only a policy can set"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_policy_value_wins_over_the_lawyers_and_locks_each_key() {
    let firm = policy(
        "firm",
        format!(
            r#"{{"sandbox": {{"required": false, "allow_unprotected_auto": true}},
                "network": {{"upstream_proxy": "https://proxy.firm.example",
                    "ca_certs": {{"codex-acp": "{}"}},
                    "allowed_domains": ["docs.firm.example", "*.Courts.example"]}}}}"#,
            abs_json("firm-ca.pem")
        ),
    );
    let effective = Effective::load(&locations(), &lawyers(), &[&firm]);
    let settings = effective.settings();
    assert!(!settings.sandbox.required);
    assert!(settings.sandbox.allow_unprotected_auto);
    assert_eq!(
        settings.network.upstream_proxy.as_deref(),
        Some("https://proxy.firm.example")
    );
    // The firm's map replaces the lawyer's whole map, never merges.
    assert_eq!(settings.network.ca_certs, certs("codex-acp", "firm-ca.pem"));
    assert_eq!(
        settings.network.allowed_domains,
        ["docs.firm.example", "*.courts.example"]
    );
    for key in KEYS {
        assert!(effective.is_locked(key), "{key}");
    }
}

/// An unreadable source locks each key at its safe value whatever the lawyer set:
/// `allow_unprotected_auto` `false` (which carries the fail-closed), `required` `false`, no
/// proxy but the OS's, no CA file, no added domain.
#[test]
fn an_unreadable_policy_source_locks_each_key_at_its_safe_value() {
    for files in [lawyers(), lawyers_auto_on(), MemFiles::default()] {
        let effective = Effective::load(&locations(), &files, &[&unreadable()]);
        let settings = effective.settings();
        assert!(!settings.sandbox.allow_unprotected_auto);
        assert!(!settings.sandbox.required);
        assert_eq!(settings.network, NetworkSettings::default());
        for key in KEYS {
            assert!(effective.is_locked(key), "{key}");
            assert!(
                effective.warnings().iter().any(|warning| matches!(
                    &warning.kind,
                    WarningKind::InvalidValue { key: k, message }
                        if k == key && message.ends_with("locked off")
                )),
                "{key}"
            );
        }
    }
    // A value a stronger, readable source set stands.
    let firm = policy("firm", r#"{"sandbox": {"allow_unprotected_auto": true}}"#);
    let effective = Effective::load(&locations(), &lawyers(), &[&firm, &unreadable()]);
    assert!(effective.settings().sandbox.allow_unprotected_auto);
    assert!(!effective.settings().sandbox.required);
}

/// A policy value that does not type locks its key at the safe value, as an unreadable source
/// does, never at the lawyer's.
#[test]
fn a_malformed_policy_value_locks_its_key_at_the_safe_value() {
    let firm = policy(
        "firm",
        r#"{"sandbox": {"required": "yes", "allow_unprotected_auto": 1},
            "network": {"upstream_proxy": "http://user:pw@proxy.firm.example",
                "ca_certs": ["x"], "allowed_domains": "docs.firm.example"}}"#,
    );
    let effective = Effective::load(&locations(), &lawyers_auto_on(), &[&firm]);
    let settings = effective.settings();
    assert!(!settings.sandbox.required);
    assert!(!settings.sandbox.allow_unprotected_auto);
    assert_eq!(settings.network, NetworkSettings::default());
    for key in KEYS {
        assert!(effective.is_locked(key), "{key}");
    }
    // No message quotes the refused address.
    assert!(
        effective
            .warnings()
            .iter()
            .all(|warning| !format!("{warning:?}").contains("pw@")),
        "{:?}",
        effective.warnings()
    );
}

#[test]
fn the_registry_sets_each_key_by_its_type() {
    let ca = format!(r#"{{"claude-acp": "{}"}}"#, abs_json("ca.pem"));
    let values = registry::parse(vec![
        ("Sandbox.Required".to_owned(), RegValue::Dword(1)),
        (
            "sandbox.allow_unprotected_auto".to_owned(),
            RegValue::Dword(0),
        ),
        (
            "network.upstream_proxy".to_owned(),
            RegValue::Sz("http://proxy.firm.example:8080".to_owned()),
        ),
        ("network.ca_certs".to_owned(), RegValue::Sz(ca)),
        (
            "network.allowed_domains".to_owned(),
            RegValue::MultiSz(vec!["docs.firm.example".to_owned()]),
        ),
    ]);
    let source = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    assert_eq!(source.name(), "registry");
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&source]);
    let settings = effective.settings();
    assert!(settings.sandbox.required);
    assert!(!settings.sandbox.allow_unprotected_auto);
    assert_eq!(
        settings.network.upstream_proxy.as_deref(),
        Some("http://proxy.firm.example:8080")
    );
    assert_eq!(settings.network.ca_certs, certs("claude-acp", "ca.pem"));
    assert_eq!(settings.network.allowed_domains, ["docs.firm.example"]);
    for key in KEYS {
        assert!(effective.is_locked(key), "{key}");
    }
}

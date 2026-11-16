//! The security keys: the `tools` switches typed, and [`refuse`](super::refuse) and
//! `Leaf::apply_policy` locking each security key off.

use super::tests::one;
use super::{Leaf, refuse};
use crate::schema::{Plan, Settings, SkillsExtra, Theme, UpdateMode};
use crate::testing::{abs, abs_str};
use crate::warning::WarningKind;
use serde_json::json;

#[test]
fn each_tools_key_is_a_boolean() {
    assert_eq!(
        one("tools.web_search", json!(false)),
        Ok(Leaf::WebSearch(false))
    );
    assert_eq!(one("tools.browser", json!(true)), Ok(Leaf::Browser(true)));
    assert_eq!(
        one("tools.connectors", json!(false)),
        Ok(Leaf::Connectors(false))
    );
    for key in ["tools.web_search", "tools.browser", "tools.connectors"] {
        for value in [json!("yes"), json!(1), json!(null), json!({})] {
            assert!(
                matches!(&one(key, value.clone()).unwrap_err()[..], [WarningKind::InvalidValue { key: k, .. }] if k == key),
                "{key}: {value}"
            );
        }
    }
}

// A security key fails closed: refused, it is locked off, not at its default, which for both
// tools keys can be on (R21).
#[test]
fn refuse_locks_each_security_key_off() {
    let on = || {
        let mut settings = Settings::default();
        settings.tools.web_search = true;
        settings.tools.browser = true;
        settings.tools.connectors = true;
        settings.privacy.counts = true;
        settings.privacy.crash_reports = true;
        settings.update.mode = UpdateMode::Notify;
        settings.skills.extra = SkillsExtra::Any;
        settings.skills.allow_scripts = true;
        settings.skills.curated = vec!["nda-kit".to_owned()];
        settings
            .skills
            .enabled
            .insert("nda-kit".to_owned(), format!("sha256:{}", "a".repeat(64)));
        settings.mcp.servers.insert(
            "s".to_owned(),
            serde_json::from_value(json!({"command": abs_str("s"), "enabled": true})).unwrap(),
        );
        settings
    };
    let mut settings = on();
    assert!(refuse("tools.web_search", &mut settings));
    assert!(!settings.tools.web_search);
    assert!(settings.tools.browser);
    assert_eq!(settings.mcp.servers.len(), 1);

    let mut settings = on();
    assert!(refuse("tools.browser", &mut settings));
    assert!(settings.tools.web_search);
    assert!(!settings.tools.browser);

    // R24: the connectors switch defaults to on, so it locks off, alone.
    let mut settings = on();
    assert!(refuse("tools.connectors", &mut settings));
    assert!(!settings.tools.connectors);
    assert!(settings.tools.web_search && settings.tools.browser);

    // The privacy keys lock off too (0036): counts defaults to on.
    let mut settings = on();
    assert!(refuse("privacy.counts", &mut settings));
    assert!(!settings.privacy.counts && settings.privacy.crash_reports);
    let mut settings = on();
    assert!(refuse("privacy.", &mut settings));
    assert!(!settings.privacy.counts && !settings.privacy.crash_reports);
    assert!(settings.tools.web_search && settings.tools.browser);

    // The update mode locks at `none` (R23): its default, `auto`, would fail open.
    let mut settings = on();
    assert!(refuse("update.mode", &mut settings));
    assert_eq!(settings.update.mode, UpdateMode::None);
    assert!(settings.privacy.counts && settings.tools.web_search);
    let mut settings = on();
    assert!(refuse("update.", &mut settings));
    assert_eq!(settings.update.mode, UpdateMode::None);

    let mut settings = on();
    assert!(refuse("mcp.servers", &mut settings));
    assert!(settings.mcp.servers.is_empty());
    assert!(settings.tools.web_search && settings.tools.browser);

    // The skills keys lock at no extra pack, no script and none curated (0040); the lawyer's
    // turned-on packs are no security key and stay.
    let mut settings = on();
    assert!(refuse("skills.extra", &mut settings));
    assert_eq!(settings.skills.extra, SkillsExtra::Off);
    assert!(settings.skills.allow_scripts && !settings.skills.curated.is_empty());
    let mut settings = on();
    assert!(refuse("skills.allow_scripts", &mut settings));
    assert!(!settings.skills.allow_scripts);
    assert_eq!(settings.skills.extra, SkillsExtra::Any);
    let mut settings = on();
    assert!(refuse("skills.curated", &mut settings));
    assert!(settings.skills.curated.is_empty() && settings.skills.allow_scripts);
    let mut settings = on();
    assert!(refuse("skills.", &mut settings));
    assert_eq!(settings.skills.extra, SkillsExtra::Off);
    assert!(!settings.skills.allow_scripts && settings.skills.curated.is_empty());
    assert_eq!(settings.skills.enabled, on().skills.enabled);
    assert!(settings.tools.web_search && settings.privacy.counts);

    // A prefix ending in `.` refuses every security key under it, and only those.
    let mut settings = on();
    assert!(refuse("tools.", &mut settings));
    assert!(!settings.tools.web_search && !settings.tools.browser && !settings.tools.connectors);
    assert_eq!(settings.mcp.servers.len(), 1);

    // Any other key, a prefix of no security key, or a prefix without its dot names nothing. A
    // single plan names none either: `reset` drops it alone.
    for key in [
        "ui.theme",
        "agents.x",
        "plans.x",
        "plans",
        "install",
        "tools",
        "tool.",
        "tools.web",
        "skills.enabled",
        "skills",
    ] {
        let mut settings = on();
        assert!(!refuse(key, &mut settings), "{key}");
        assert_eq!(settings, on(), "{key}");
    }
}

#[test]
fn a_policy_refusal_locks_a_security_key_off_and_any_other_at_its_default() {
    let mut settings = Settings::default();
    settings.tools.web_search = true;
    settings.tools.browser = true;
    settings.ui.theme = Theme::Dark;
    for key in ["tools.web_search", "tools.browser", "ui.theme"] {
        Leaf::Refused(key.to_owned()).apply_policy(&mut settings);
    }
    assert!(!settings.tools.web_search);
    assert!(!settings.tools.browser);
    assert_eq!(settings.ui.theme, Theme::default());
    // A value from policy applies as any value does.
    Leaf::WebSearch(true).apply_policy(&mut settings);
    assert!(settings.tools.web_search);
}

// The `install` keys lock unset and `plans.` locks no plan at all (decision record 0048); a
// single refused plan drops that plan alone.
#[test]
fn refuse_unsets_the_install_keys_and_leaves_no_plan() {
    let set = || {
        let mut settings = Settings::default();
        settings.install.bundle = Some(abs("bundle"));
        settings.install.registry = Some("https://npm.firm.example".to_owned());
        for id in ["claude-acp", "codex-acp"] {
            settings.plans.insert(id.to_owned(), Plan::default());
        }
        settings
    };
    let mut settings = set();
    assert!(refuse("install.bundle", &mut settings));
    assert_eq!(settings.install.bundle, None);
    assert!(settings.install.registry.is_some());
    let mut settings = set();
    assert!(refuse("install.", &mut settings));
    assert_eq!(settings.install, crate::schema::InstallSettings::default());
    assert_eq!(settings.plans.len(), 2);
    let mut settings = set();
    assert!(refuse("plans.", &mut settings));
    assert!(settings.plans.is_empty());
    assert!(settings.install.bundle.is_some());
    let mut settings = set();
    Leaf::Refused("plans.claude-acp".to_owned()).apply_policy(&mut settings);
    assert_eq!(settings.plans.keys().collect::<Vec<_>>(), ["codex-acp"]);
    let mut settings = set();
    Leaf::Refused("install.registry".to_owned()).apply_policy(&mut settings);
    assert_eq!(settings.install.registry, None);
    assert_eq!(abs_str("bundle"), abs("bundle").display().to_string());
}

// Decision record 0051: refused, `allow_unprotected_auto` locks at `false` though its default is
// on, `required` at its default `false`, and the `network` keys unset or empty; a refused value
// from `settings.json` goes back to each default, except `allow_unprotected_auto`, which goes to
// `false` there too.
#[test]
fn refuse_locks_the_sandbox_and_network_keys_at_their_safe_values() {
    let set = || {
        let mut settings = Settings::default();
        settings.sandbox.required = true;
        settings.sandbox.allow_unprotected_auto = true;
        settings.network.upstream_proxy = Some("http://proxy.firm.example:8080".to_owned());
        settings
            .network
            .ca_certs
            .insert("claude-acp".to_owned(), abs("ca.pem"));
        settings.network.allowed_domains = vec!["docs.firm.example".to_owned()];
        settings
    };
    let mut settings = set();
    assert!(refuse("sandbox.allow_unprotected_auto", &mut settings));
    assert!(!settings.sandbox.allow_unprotected_auto);
    assert!(settings.sandbox.required);
    let mut settings = set();
    assert!(refuse("sandbox.", &mut settings));
    assert!(!settings.sandbox.required && !settings.sandbox.allow_unprotected_auto);
    assert!(settings.network.upstream_proxy.is_some());
    let mut settings = set();
    assert!(refuse("network.", &mut settings));
    let locked = crate::schema::NetworkSettings {
        allow_unsafe_site_override: false,
        ..crate::schema::NetworkSettings::default()
    };
    assert_eq!(settings.network, locked);
    assert!(settings.sandbox.required);
    let mut settings = set();
    Leaf::Refused("sandbox.allow_unprotected_auto".to_owned()).apply_policy(&mut settings);
    assert!(!settings.sandbox.allow_unprotected_auto);
    let mut settings = set();
    Leaf::Refused("sandbox.allow_unprotected_auto".to_owned()).apply(&mut settings);
    assert!(!settings.sandbox.allow_unprotected_auto);
    for key in ["sandbox", "network", "sandbox.require", "networks."] {
        let mut settings = set();
        assert!(!refuse(key, &mut settings), "{key}");
        assert_eq!(settings, set(), "{key}");
    }
}

// Decision record 0053: allowing a blocked site is on by default; refused, or a policy value that
// does not type, locks it off; a mistyped `settings.json` value goes back to its default, on.
#[test]
fn refuse_locks_the_unsafe_site_override_off_and_reset_restores_it() {
    let key = "network.allow_unsafe_site_override";
    assert!(Settings::default().network.allow_unsafe_site_override);
    assert!(crate::leaf::SECURITY_KEYS.contains(&key));
    assert!(!crate::leaf::POLICY_ONLY_KEYS.contains(&key));
    let mut settings = Settings::default();
    assert!(refuse(key, &mut settings));
    assert!(!settings.network.allow_unsafe_site_override);
    let mut settings = Settings::default();
    Leaf::Refused(key.to_owned()).apply_policy(&mut settings);
    assert!(!settings.network.allow_unsafe_site_override);
    let mut settings = Settings::default();
    settings.network.allow_unsafe_site_override = false;
    Leaf::Refused(key.to_owned()).apply(&mut settings);
    assert!(settings.network.allow_unsafe_site_override);
}

// Decision record 0053: block list updates are on by default and set by policy only; refused,
// a policy value that does not type, or a mistyped value all leave them on, their default, so an
// unreadable policy never stops protection updates.
#[test]
fn list_updates_are_policy_only_and_lock_on() {
    let key = "network.unsafe_site_list_updates";
    assert!(Settings::default().network.unsafe_site_list_updates);
    assert!(crate::leaf::SECURITY_KEYS.contains(&key));
    assert!(crate::leaf::POLICY_ONLY_KEYS.contains(&key));
    let off = || {
        let mut settings = Settings::default();
        settings.network.unsafe_site_list_updates = false;
        settings
    };
    let mut settings = off();
    assert!(refuse(key, &mut settings));
    assert!(settings.network.unsafe_site_list_updates);
    let mut settings = off();
    Leaf::Refused(key.to_owned()).apply_policy(&mut settings);
    assert!(settings.network.unsafe_site_list_updates);
    let mut settings = off();
    Leaf::Refused(key.to_owned()).apply(&mut settings);
    assert!(settings.network.unsafe_site_list_updates);
}

//! The security keys: the `tools` switches typed, and [`refuse`](super::refuse) and
//! `Leaf::apply_policy` locking each security key off.

use super::tests::one;
use super::{Leaf, refuse};
use crate::schema::{Settings, Theme};
use crate::testing::abs_str;
use crate::warning::WarningKind;
use serde_json::json;

#[test]
fn each_tools_key_is_a_boolean() {
    assert_eq!(
        one("tools.web_search", json!(false)),
        Ok(Leaf::WebSearch(false))
    );
    assert_eq!(one("tools.browser", json!(true)), Ok(Leaf::Browser(true)));
    for key in ["tools.web_search", "tools.browser"] {
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
        settings.privacy.counts = true;
        settings.privacy.crash_reports = true;
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

    // The privacy keys lock off too (0036): counts defaults to on.
    let mut settings = on();
    assert!(refuse("privacy.counts", &mut settings));
    assert!(!settings.privacy.counts && settings.privacy.crash_reports);
    let mut settings = on();
    assert!(refuse("privacy.", &mut settings));
    assert!(!settings.privacy.counts && !settings.privacy.crash_reports);
    assert!(settings.tools.web_search && settings.tools.browser);

    let mut settings = on();
    assert!(refuse("mcp.servers", &mut settings));
    assert!(settings.mcp.servers.is_empty());
    assert!(settings.tools.web_search && settings.tools.browser);

    // A prefix ending in `.` refuses every security key under it, and only those.
    let mut settings = on();
    assert!(refuse("tools.", &mut settings));
    assert!(!settings.tools.web_search && !settings.tools.browser);
    assert_eq!(settings.mcp.servers.len(), 1);

    // Any other key, a prefix of no security key, or a prefix without its dot names nothing.
    for key in [
        "ui.theme",
        "agents.x",
        "plans.",
        "tools",
        "tool.",
        "tools.web",
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

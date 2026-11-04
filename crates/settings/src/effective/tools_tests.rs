//! `tools.web_search` and `tools.browser`: the lawyer's switches, the firm's lock on each, and
//! the fail-closed rule for both security keys (R21).

use super::Effective;
use crate::error::Error;
use crate::policy::PolicyRead;
use crate::schema::ToolsSettings;
use crate::testing::{FakeSource, MemFiles, abs, abs_json, locations, policy, user};
use crate::warning::{Origin, WarningKind};

const TOOLS: [&str; 2] = ["tools.web_search", "tools.browser"];

/// The value of the tools key `key`.
fn tool(effective: &Effective, key: &str) -> bool {
    let tools = &effective.settings().tools;
    match key {
        "tools.web_search" => tools.web_search,
        "tools.browser" => tools.browser,
        other => panic!("{other}"),
    }
}

/// `settings.json` with both tools on and one MCP server on.
fn all_on() -> MemFiles {
    user(&format!(
        r#"{{"tools": {{"web_search": true, "browser": true}},
            "mcp": {{"servers": {{"mine": {{"command": "{}", "enabled": true}}}}}}}}"#,
        abs_json("bin/mine")
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

#[test]
fn the_defaults_load_unlocked() {
    let effective = Effective::load(&locations(), &MemFiles::default(), &[]);
    assert_eq!(effective.settings().tools, ToolsSettings::default());
    for key in TOOLS {
        assert!(!effective.is_locked(key), "{key}");
    }
}

#[test]
fn the_users_switches_load_unlocked() {
    let files = user(r#"{"tools": {"web_search": false, "browser": true}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.warnings(), []);
    assert!(!effective.settings().tools.web_search);
    assert!(effective.settings().tools.browser);
    assert_eq!(effective.locked().count(), 0);
}

// The user's own file follows the rule for every key: a value that does not type goes back to
// the default, with a warning, and locks nothing. Only policy fails closed.
#[test]
fn a_users_value_of_the_wrong_type_goes_back_to_its_default() {
    let files = user(r#"{"tools": {"web_search": "no", "browser": 0}}"#);
    let effective = Effective::load(&locations(), &files, &[]);
    assert_eq!(effective.settings().tools, ToolsSettings::default());
    assert_eq!(effective.warnings().len(), 2);
    assert_eq!(effective.locked().count(), 0);
}

#[test]
fn an_unreadable_policy_source_locks_web_search_off() {
    let effective = Effective::load(&locations(), &all_on(), &[&unreadable()]);
    assert!(!effective.settings().tools.web_search);
    assert!(effective.is_locked("tools.web_search"));
}

#[test]
fn an_unreadable_policy_source_locks_the_browser_off() {
    let effective = Effective::load(&locations(), &all_on(), &[&unreadable()]);
    assert!(!effective.settings().tools.browser);
    assert!(effective.is_locked("tools.browser"));
}

#[test]
fn an_unreadable_policy_source_locks_every_security_key_off_with_a_warning_each() {
    let effective = Effective::load(&locations(), &all_on(), &[&unreadable()]);
    assert!(effective.mcp_servers().is_empty());
    assert_eq!(
        effective.locked().collect::<Vec<_>>(),
        [
            "install.bundle",
            "install.registry",
            "mcp.servers",
            "plans.",
            "privacy.counts",
            "privacy.crash_reports",
            "tools.browser",
            "tools.web_search",
            "update.mode"
        ]
    );
    let keys: Vec<&str> = effective
        .warnings()
        .iter()
        .map(|w| {
            assert_eq!(
                w.origin,
                Origin::Store {
                    name: "broken".to_owned()
                }
            );
            match &w.kind {
                WarningKind::InvalidValue { key, message } => {
                    assert!(message.ends_with("locked off"), "{message}");
                    key.as_str()
                }
                other => panic!("{other:?}"),
            }
        })
        .collect();
    assert_eq!(
        keys,
        [
            "mcp.servers",
            "tools.web_search",
            "tools.browser",
            "privacy.counts",
            "privacy.crash_reports",
            "update.mode",
            "install.bundle",
            "install.registry",
            "plans."
        ]
    );
}

#[test]
fn a_stronger_policy_value_stands_over_a_weaker_unreadable_source() {
    let firm = policy("firm", r#"{"tools": {"web_search": true}}"#);
    let effective = Effective::load(&locations(), &all_on(), &[&firm, &unreadable()]);
    assert!(effective.settings().tools.web_search);
    assert!(!effective.settings().tools.browser);
    assert!(effective.is_locked("tools.web_search"));
    assert!(effective.is_locked("tools.browser"));
}

#[test]
fn a_policy_value_of_the_wrong_type_locks_web_search_off() {
    let firm = policy("firm", r#"{"tools": {"web_search": "yes"}}"#);
    let effective = Effective::load(&locations(), &all_on(), &[&firm]);
    assert!(!effective.settings().tools.web_search);
    assert!(effective.is_locked("tools.web_search"));
    // The other key is the user's, unlocked.
    assert!(effective.settings().tools.browser);
    assert!(!effective.is_locked("tools.browser"));
    assert!(matches!(
        effective.warnings(),
        [w] if matches!(&w.kind, WarningKind::InvalidValue { key, .. } if key == "tools.web_search")
    ));
}

#[test]
fn a_policy_value_of_the_wrong_type_locks_the_browser_off() {
    let firm = policy("firm", r#"{"tools": {"browser": 1}}"#);
    let effective = Effective::load(&locations(), &all_on(), &[&firm]);
    assert!(!effective.settings().tools.browser);
    assert!(effective.is_locked("tools.browser"));
    assert!(effective.settings().tools.web_search);
    assert!(!effective.is_locked("tools.web_search"));
}

#[test]
fn a_tools_section_policy_cannot_read_locks_both_off() {
    for text in [
        r#"{"tools": "off"}"#,
        r#"{"tools": {"brwser": true}}"#,
        r#"{"Tools": true}"#,
        // A section of another spelling is not read inside: what it holds is unknown.
        r#"{"Tools": {"browser": true}}"#,
        r#"{"tools": {"web_search": "yes", "brwser": true}}"#,
    ] {
        let firm = policy("firm", text);
        let effective = Effective::load(&locations(), &all_on(), &[&firm]);
        for key in TOOLS {
            assert!(!tool(&effective, key), "{text}: {key}");
            assert!(effective.is_locked(key), "{text}: {key}");
        }
    }
}

/// A misnamed `tools` key refuses only the key it could stand for; one that could stand for
/// either spares a sibling the policy sets validly by its exact name, which keeps its value.
#[test]
fn a_misnamed_tools_key_refuses_only_what_it_could_stand_for() {
    // (policy, web_search's value, browser's value, the locked keys)
    let cases: [(&str, bool, bool, &[&str]); 6] = [
        (
            r#"{"tools": {"websearch": true}}"#,
            false,
            true,
            &["tools.web_search"],
        ),
        (
            r#"{"tools": {"Browser": true}}"#,
            true,
            false,
            &["tools.browser"],
        ),
        (
            r#"{"tools": {"Web-Search": 1}}"#,
            false,
            true,
            &["tools.web_search"],
        ),
        (
            r#"{"tools": {"web_search": true, "brwser": true}}"#,
            true,
            false,
            &["tools.web_search", "tools.browser"],
        ),
        (
            r#"{"tools": {"browser": true, "brwser": false}}"#,
            false,
            true,
            &["tools.web_search", "tools.browser"],
        ),
        (
            r#"{"tools": {"web_search": true, "websearch": false}}"#,
            false,
            true,
            &["tools.web_search"],
        ),
    ];
    for (text, web_search, browser, locked) in cases {
        let firm = policy("firm", text);
        let effective = Effective::load(&locations(), &all_on(), &[&firm]);
        assert_eq!(tool(&effective, "tools.web_search"), web_search, "{text}");
        assert_eq!(tool(&effective, "tools.browser"), browser, "{text}");
        for key in TOOLS {
            assert_eq!(
                effective.is_locked(key),
                locked.contains(&key),
                "{text}: {key}"
            );
        }
    }
}

#[test]
fn each_tools_key_set_in_policy_locks_it_alone() {
    for key in TOOLS {
        let name = key.strip_prefix("tools.").unwrap();
        for value in [true, false] {
            let firm = policy("firm", format!(r#"{{"tools": {{"{name}": {value}}}}}"#));
            let files = user(r#"{"tools": {"web_search": false, "browser": false}}"#);
            let flipped = Effective::load(&locations(), &files, &[&firm]);
            let effective = Effective::load(&locations(), &all_on(), &[&firm]);
            for effective in [&flipped, &effective] {
                assert_eq!(tool(effective, key), value, "{key}={value}");
                assert_eq!(
                    effective.locked().collect::<Vec<_>>(),
                    [key],
                    "{key}={value}"
                );
            }
            assert_eq!(effective.warnings(), []);
        }
    }
}

// The reserved name is dropped from either source; the rest of the list loads, `clauseo` among
// them, and a policy list still locks.
#[test]
fn the_reserved_mcp_server_name_is_dropped_from_settings_json() {
    let text = format!(
        r#"{{"mcp": {{"servers": {{
            "browser": {{"command": "{b}", "enabled": true}},
            "clauseo": {{"command": "{c}", "enabled": true}}
        }}}}}}"#,
        b = abs_json("bin/browser"),
        c = abs_json("bin/clauseo"),
    );
    let effective = Effective::load(&locations(), &user(&text), &[]);
    assert_eq!(
        effective.mcp_servers().keys().collect::<Vec<_>>(),
        ["clauseo"]
    );
    assert!(!effective.is_locked("mcp.servers"));
    assert!(matches!(
        effective.warnings(),
        [w] if matches!(&w.kind, WarningKind::InvalidValue { key, message }
            if key == "mcp.servers.browser" && message.contains("reserved"))
    ));
}

#[test]
fn the_reserved_mcp_server_name_is_dropped_from_policy_and_the_list_still_locks() {
    let text = format!(
        r#"{{"mcp": {{"servers": {{
            "browser": {{"command": "{b}", "enabled": true}},
            "clauseo": {{"command": "{c}", "enabled": true}}
        }}}}}}"#,
        b = abs_json("bin/browser"),
        c = abs_json("bin/clauseo"),
    );
    let firm = policy("firm", text);
    let effective = Effective::load(&locations(), &all_on(), &[&firm]);
    assert_eq!(
        effective
            .enabled_mcp_servers()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        ["clauseo"]
    );
    assert!(effective.is_locked("mcp.servers"));
    assert!(matches!(
        effective.warnings(),
        [w] if matches!(&w.kind, WarningKind::InvalidValue { key, .. } if key == "mcp.servers.browser")
    ));
}

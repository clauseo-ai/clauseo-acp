//! `mcp.servers`: the list a lawyer turns servers on in, and the firm's lock on it.

use super::Effective;
use crate::error::Error;
use crate::policy::PolicyRead;
use crate::schema::McpServerLaunch;
use crate::testing::{FakeSource, MemFiles, abs, abs_json, locations, policy, user};
use crate::warning::Origin;
use crate::warning::WarningKind;
use std::collections::BTreeMap;

/// A server at `bin/<name>`, on or off.
fn server(name: &str, enabled: bool) -> McpServerLaunch {
    McpServerLaunch {
        command: abs(&format!("bin/{name}")).display().to_string(),
        args: Vec::new(),
        env: BTreeMap::new(),
        enabled,
    }
}

/// `{"mcp": {"servers": {...}}}` with each `(name, enabled)` at `bin/<name>`.
fn servers_json(servers: &[(&str, bool)]) -> String {
    let entries: Vec<String> = servers
        .iter()
        .map(|(name, enabled)| {
            format!(
                r#""{name}": {{"command": "{}", "enabled": {enabled}}}"#,
                abs_json(&format!("bin/{name}"))
            )
        })
        .collect();
    format!(r#"{{"mcp": {{"servers": {{{}}}}}}}"#, entries.join(", "))
}

fn enabled(effective: &Effective) -> Vec<&str> {
    effective
        .enabled_mcp_servers()
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn no_servers_by_default() {
    let effective = Effective::load(&locations(), &MemFiles::default(), &[]);
    assert!(effective.mcp_servers().is_empty());
    assert_eq!(enabled(&effective), Vec::<&str>::new());
    assert!(!effective.is_locked("mcp.servers"));
}

#[test]
fn a_server_is_off_until_turned_on_and_the_enabled_come_in_name_order() {
    let text = format!(
        r#"{{"mcp": {{"servers": {{
            "zeta": {{"command": "{z}", "enabled": true}},
            "beta": {{"command": "{b}"}},
            "alpha": {{"command": "{a}", "args": ["--stdio"], "env": {{"K": "v"}}, "enabled": true}}
        }}}}}}"#,
        z = abs_json("bin/zeta"),
        b = abs_json("bin/beta"),
        a = abs_json("bin/alpha"),
    );
    let effective = Effective::load(&locations(), &user(&text), &[]);
    assert_eq!(effective.warnings(), []);
    assert_eq!(
        effective.mcp_servers().keys().collect::<Vec<_>>(),
        ["alpha", "beta", "zeta"]
    );
    assert_eq!(
        effective.mcp_servers().get("beta"),
        Some(&server("beta", false))
    );
    assert_eq!(enabled(&effective), ["alpha", "zeta"]);
    let alpha = effective.mcp_servers().get("alpha").unwrap();
    assert_eq!(alpha.args, ["--stdio"]);
    assert_eq!(
        alpha.env,
        BTreeMap::from([("K".to_owned(), "v".to_owned())])
    );
    assert!(!effective.is_locked("mcp.servers"));
}

#[test]
fn a_policy_list_replaces_the_users_whole_list_and_locks_it() {
    let files = user(&servers_json(&[("mine", true), ("shared", true)]));
    let firm = policy("firm", servers_json(&[("firm", true), ("shared", false)]));
    let effective = Effective::load(&locations(), &files, &[&firm]);
    assert_eq!(
        effective.mcp_servers(),
        &BTreeMap::from([
            ("firm".to_owned(), server("firm", true)),
            ("shared".to_owned(), server("shared", false)),
        ])
    );
    assert_eq!(enabled(&effective), ["firm"]);
    assert!(effective.is_locked("mcp.servers"));
    assert_eq!(effective.policy().applied(), ["firm"]);
}

#[test]
fn an_empty_policy_list_locks_every_server_off() {
    let files = user(&servers_json(&[("mine", true)]));
    let firm = policy("firm", r#"{"mcp": {"servers": {}}}"#);
    let effective = Effective::load(&locations(), &files, &[&firm]);
    assert!(effective.mcp_servers().is_empty());
    assert!(effective.is_locked("mcp.servers"));
}

// Fail closed (0009): a policy list that is not an object locks the list at its default, empty.
#[test]
fn an_invalid_policy_list_locks_it_empty_with_a_warning() {
    let files = user(&servers_json(&[("mine", true)]));
    for value in [r#""all""#, "[]", "null", "true"] {
        let firm = policy("firm", format!(r#"{{"mcp": {{"servers": {value}}}}}"#));
        let effective = Effective::load(&locations(), &files, &[&firm]);
        assert!(effective.mcp_servers().is_empty(), "{value}");
        assert_eq!(enabled(&effective), Vec::<&str>::new(), "{value}");
        assert!(effective.is_locked("mcp.servers"), "{value}");
        let kinds: Vec<_> = effective
            .warnings()
            .iter()
            .map(|w| w.kind.clone())
            .collect();
        assert!(
            matches!(&kinds[..], [WarningKind::InvalidValue { key, .. }] if key == "mcp.servers"),
            "{value}: {kinds:?}"
        );
    }
}

// The user's broken list is refused too, but locks nothing.
#[test]
fn an_invalid_user_list_is_empty_and_unlocked() {
    let effective = Effective::load(&locations(), &user(r#"{"mcp": {"servers": []}}"#), &[]);
    assert!(effective.mcp_servers().is_empty());
    assert!(!effective.is_locked("mcp.servers"));
    assert_eq!(effective.warnings().len(), 1);
}

#[test]
fn a_bad_entry_in_a_policy_list_is_dropped_and_the_list_still_locks() {
    let files = user(&servers_json(&[("mine", true)]));
    let text = format!(
        r#"{{"mcp": {{"servers": {{
            "firm": {{"command": "{f}", "enabled": true}},
            "relative": {{"command": "server", "enabled": true}},
            "bad name": {{"command": "{f}", "enabled": true}}
        }}}}}}"#,
        f = abs_json("bin/firm"),
    );
    let firm = policy("firm", text);
    let effective = Effective::load(&locations(), &files, &[&firm]);
    assert_eq!(enabled(&effective), ["firm"]);
    assert!(effective.is_locked("mcp.servers"));
    assert_eq!(effective.warnings().len(), 2, "{:?}", effective.warnings());
}

#[test]
fn the_mcp_section_must_be_an_object() {
    let effective = Effective::load(&locations(), &user(r#"{"mcp": []}"#), &[]);
    assert!(effective.mcp_servers().is_empty());
    assert!(matches!(
        effective.warnings(),
        [w] if matches!(&w.kind, WarningKind::InvalidValue { key, .. } if key == "mcp")
    ));
}

// A policy that names the `mcp` section in a form that does not set it fails closed: the
// firm's list there is unknown, so the user's must not stand in (0009, review of #117).
#[test]
fn a_malformed_or_misnamed_policy_mcp_section_locks_the_list_empty() {
    let files = user(&servers_json(&[("mine", true)]));
    let key_case = format!(
        r#"{{"mcp": {{"Servers": {{"firm": {{"command": "{}", "enabled": true}}}}}}}}"#,
        abs_json("bin/firm")
    );
    let texts = [
        r#"{"mcp": "off"}"#.to_owned(),
        r#"{"mcp": []}"#.to_owned(),
        r#"{"mcp": null}"#.to_owned(),
        r#"{"mcp": false}"#.to_owned(),
        r#"{"MCP": {"servers": {}}}"#.to_owned(),
        key_case,
    ];
    for text in texts {
        let firm = policy("firm", text.clone());
        let effective = Effective::load(&locations(), &files, &[&firm]);
        assert!(effective.mcp_servers().is_empty(), "{text}");
        assert!(effective.is_locked("mcp.servers"), "{text}");
        assert!(!effective.warnings().is_empty(), "{text}");
    }
    // A stray case variant beside a valid section still locks it empty: fail closed.
    let both = format!(
        r#"{{"mcp": {{"servers": {{"firm": {{"command": "{}", "enabled": true}}}}}}, "Mcp": 1}}"#,
        abs_json("bin/firm")
    );
    let effective = Effective::load(&locations(), &files, &[&policy("firm", both)]);
    assert!(effective.mcp_servers().is_empty());
    assert!(effective.is_locked("mcp.servers"));
    // Another unknown section locks nothing.
    let other = policy("firm", r#"{"mcpx": {}, "ui": {"colour": "red"}}"#);
    let effective = Effective::load(&locations(), &files, &[&other]);
    assert_eq!(enabled(&effective), ["mine"]);
    assert!(!effective.is_locked("mcp.servers"));
}

// The user's own misnamed section is only a warning: nothing locks, and their list stands.
#[test]
fn a_misnamed_user_mcp_section_locks_nothing() {
    let mut text = servers_json(&[("mine", true)]);
    text.insert_str(text.len() - 1, r#", "MCP": 1"#);
    let effective = Effective::load(&locations(), &user(&text), &[]);
    assert_eq!(enabled(&effective), ["mine"]);
    assert!(!effective.is_locked("mcp.servers"));
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

// A security-relevant key fails closed on an unreadable source too (0009, 2026-11-02).
#[test]
fn an_unreadable_policy_source_locks_the_list_empty() {
    let files = user(&servers_json(&[("mine", true)]));
    let effective = Effective::load(&locations(), &files, &[&unreadable()]);
    assert!(effective.mcp_servers().is_empty());
    assert!(effective.is_locked("mcp.servers"));
    assert_eq!(effective.policy().errors().len(), 1);
    assert!(effective.policy().is_absent());
    assert!(matches!(
        effective.warnings(),
        [w] if w.origin == Origin::Store { name: "broken".to_owned() }
            && matches!(&w.kind, WarningKind::InvalidValue { key, .. } if key == "mcp.servers")
    ));
    // Other keys stay the user's: only security-relevant keys lock.
    assert_eq!(effective.locked().collect::<Vec<_>>(), ["mcp.servers"]);
}

#[test]
fn an_unreadable_source_after_a_list_leaves_the_stronger_list() {
    let firm = policy("firm", servers_json(&[("firm", true)]));
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&firm, &unreadable()]);
    assert_eq!(enabled(&effective), ["firm"]);
    assert_eq!(effective.warnings(), []);
    // A weaker readable list does not win over the stronger unreadable source.
    let weak = policy("weak", servers_json(&[("weak", true)]));
    let effective = Effective::load(&locations(), &MemFiles::default(), &[&unreadable(), &weak]);
    assert!(effective.mcp_servers().is_empty());
    assert!(effective.is_locked("mcp.servers"));
}

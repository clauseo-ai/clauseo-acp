//! `mcp.servers`: the list typed entry by entry, a list that is not an object, and the reserved
//! name.

use super::tests::{invalid, one, origin};
use super::{Leaf, RESERVED_MCP_SERVERS, Raw, reserved_mcp_server, typed};
use crate::schema::{McpServerLaunch, Settings};
use crate::testing::abs_str;
use crate::warning::WarningKind;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn mcp_servers_type_entry_by_entry() {
    let command = abs_str("bin/s");
    let mut value = json!({
            "ok": {"command": command, "args": ["-v"], "env": {"A": "1"}, "enabled": true, "url": "x"},
            "off_by-default_9": {"command": command},
            "relative": {"command": "server"},
            "no-command": {"args": []},
            "enabled-not-bool": {"command": command, "enabled": "yes"},
            "": {"command": command},
            "has space": {"command": command},
            "é": {"command": command},
    });
    for length in [64, 65] {
        value["a".repeat(length)] = json!({"command": command});
    }
    let mut warnings = Vec::new();
    let raw = Raw {
        key: "mcp.servers".to_owned(),
        value,
        origin: origin(),
    };
    let leaves = typed(vec![raw], Vec::new(), &mut warnings);
    let Some(Leaf::McpServers(servers)) = leaves.first() else {
        panic!("{leaves:?}");
    };
    assert_eq!(
        servers.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a".repeat(64).as_str(), "off_by-default_9", "ok"]
    );
    assert_eq!(
        servers.get("ok"),
        Some(&McpServerLaunch {
            command,
            args: vec!["-v".to_owned()],
            env: BTreeMap::from([("A".to_owned(), "1".to_owned())]),
            enabled: true,
        })
    );
    assert!(!servers.get("off_by-default_9").unwrap().enabled);
    let mut keys: Vec<String> = warnings
        .into_iter()
        .map(|warning| match warning.kind {
            WarningKind::InvalidValue { key, .. } | WarningKind::UnknownKey { key } => key,
            other => panic!("{other:?}"),
        })
        .collect();
    keys.sort();
    let mut expected: Vec<String> = [
        "",
        "a".repeat(65).as_str(),
        "enabled-not-bool",
        "has space",
        "no-command",
        "ok.url",
        "relative",
        "é",
    ]
    .iter()
    .map(|name| format!("mcp.servers.{name}"))
    .collect();
    expected.sort();
    assert_eq!(keys, expected);
}

#[test]
fn an_mcp_list_that_is_not_an_object_is_refused() {
    for value in [json!([]), json!("all"), json!(null)] {
        assert_eq!(
            one("mcp.servers", value.clone()),
            Err(invalid(
                "mcp.servers",
                "it must be an object from server name to server"
            )),
            "{value}"
        );
    }
    let mut settings = Settings::default();
    settings.mcp.servers.insert(
        "s".to_owned(),
        serde_json::from_value(json!({"command": abs_str("s")})).unwrap(),
    );
    Leaf::Refused("mcp.servers".to_owned()).apply(&mut settings);
    assert!(settings.mcp.servers.is_empty());
}

#[test]
fn the_reserved_mcp_server_name_is_left_out_and_the_rest_load() {
    assert_eq!(
        RESERVED_MCP_SERVERS.map(|(name, _)| name),
        ["browser", "case-law"]
    );
    let command = abs_str("bin/s");
    let value = json!({
        "browser": {"command": command, "enabled": true},
        "Browser": {"command": command, "enabled": true},
        "clauseo": {"command": command, "enabled": true},
        "other": {"command": command},
    });
    let mut warnings = Vec::new();
    let raw = Raw {
        key: "mcp.servers".to_owned(),
        value,
        origin: origin(),
    };
    let leaves = typed(vec![raw], Vec::new(), &mut warnings);
    let Some(Leaf::McpServers(servers)) = leaves.first() else {
        panic!("{leaves:?}");
    };
    assert_eq!(
        servers.keys().map(String::as_str).collect::<Vec<_>>(),
        ["clauseo", "other"]
    );
    let mut keys: Vec<String> = warnings
        .into_iter()
        .map(|warning| match warning.kind {
            WarningKind::InvalidValue { key, message } => {
                assert!(message.contains("browser is reserved"), "{message}");
                key
            }
            other => panic!("{other:?}"),
        })
        .collect();
    keys.sort();
    assert_eq!(keys, ["mcp.servers.Browser", "mcp.servers.browser"]);
}

/// R24: `case-law`, the case-law connector's relay, is reserved like `browser`, in any case, so
/// a firm's or the lawyer's server of that name never takes the place of Clauseo's entry or
/// inherits its pre-allow. Near names are not reserved.
#[test]
fn the_case_law_name_is_reserved_too() {
    assert_eq!(
        reserved_mcp_server("Case-Law"),
        Some(("case-law", "Clauseo's case-law connector"))
    );
    for near in ["case_law", "caselaw", "case-law-2", "my-case-law"] {
        assert_eq!(reserved_mcp_server(near), None, "{near}");
    }
    let command = abs_str("bin/s");
    let raw = Raw {
        key: "mcp.servers".to_owned(),
        value: json!({
            "case-law": {"command": command, "enabled": true},
            "CASE-LAW": {"command": command, "enabled": true},
            "case_law": {"command": command, "enabled": true},
        }),
        origin: origin(),
    };
    let mut warnings = Vec::new();
    let leaves = typed(vec![raw], Vec::new(), &mut warnings);
    let Some(Leaf::McpServers(servers)) = leaves.first() else {
        panic!("{leaves:?}");
    };
    assert_eq!(
        servers.keys().map(String::as_str).collect::<Vec<_>>(),
        ["case_law"]
    );
    let mut keys: Vec<String> = warnings
        .into_iter()
        .map(|warning| match warning.kind {
            WarningKind::InvalidValue { key, message } => {
                assert_eq!(
                    message,
                    "the server name case-law is reserved for Clauseo's case-law connector"
                );
                key
            }
            other => panic!("{other:?}"),
        })
        .collect();
    keys.sort();
    assert_eq!(keys, ["mcp.servers.CASE-LAW", "mcp.servers.case-law"]);
}

/// Claude names a tool `mcp__<server>__<tool>` and splits it at the first `__` after `mcp__`, so
/// a server holding `__` (`case-law__x`) or ending in `_` (`case-law_`, tool
/// `mcp__case-law___x`) would read as `case-law` and inherit its pre-allow. Every such name is
/// left out with a warning that quotes it; the reserved names match in any case; near names, and
/// a leading `_`, load.
#[test]
fn a_double_underscore_or_a_reserved_name_in_any_case_is_left_out() {
    let command = abs_str("bin/s");
    let names = [
        "case-law__x",
        "browser__x",
        "a__b",
        "Case-Law",
        "BROWSER",
        "case-law-notes",
        "my_browser",
        "case-law_",
        "browser_",
        "Case-Law_",
        "_browser",
    ];
    let value: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .map(|name| ((*name).to_owned(), json!({"command": command})))
        .collect();
    let raw = Raw {
        key: "mcp.servers".to_owned(),
        value: value.into(),
        origin: origin(),
    };
    let mut warnings = Vec::new();
    let leaves = typed(vec![raw], Vec::new(), &mut warnings);
    let Some(Leaf::McpServers(servers)) = leaves.first() else {
        panic!("{leaves:?}");
    };
    assert_eq!(
        servers.keys().map(String::as_str).collect::<Vec<_>>(),
        ["_browser", "case-law-notes", "my_browser"]
    );
    let mut found: Vec<(String, String)> = warnings
        .into_iter()
        .map(|warning| match warning.kind {
            WarningKind::InvalidValue { key, message } => (key, message),
            other => panic!("{other:?}"),
        })
        .collect();
    found.sort();
    let double = |name: &str| {
        (
            format!("mcp.servers.{name}"),
            format!(
                "double underscores and a trailing underscore are not allowed in server \
                 names ({name})"
            ),
        )
    };
    assert_eq!(
        found,
        [
            (
                "mcp.servers.BROWSER".to_owned(),
                "the server name browser is reserved for Clauseo's own browser".to_owned()
            ),
            (
                "mcp.servers.Case-Law".to_owned(),
                "the server name case-law is reserved for Clauseo's case-law connector".to_owned()
            ),
            double("Case-Law_"),
            double("a__b"),
            double("browser_"),
            double("browser__x"),
            double("case-law_"),
            double("case-law__x"),
        ]
    );
}

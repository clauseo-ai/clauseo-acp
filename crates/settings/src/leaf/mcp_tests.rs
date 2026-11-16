//! `mcp.servers`: the list typed entry by entry, a list that is not an object, and the reserved
//! name.

use super::tests::{invalid, one, origin};
use super::{Leaf, RESERVED_MCP_SERVERS, Raw, reserved_mcp_server, typed};
use crate::schema::{McpServerLaunch, McpTransport, McpValue, Settings};
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
            "relative": {"command": "bin/server"},
            "bare": {"command": "npx"},
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
        ["a".repeat(64).as_str(), "bare", "off_by-default_9", "ok"]
    );
    assert_eq!(
        servers.get("ok"),
        Some(&McpServerLaunch::local(
            command,
            vec!["-v".to_owned()],
            BTreeMap::from([("A".to_owned(), McpValue::Plain("1".to_owned()))]),
            true,
        ))
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
        [
            "browser",
            "case-law",
            "ide",
            "claude-in-chrome",
            "computer-use"
        ]
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

/// The servers and the warnings' keys `value` types to.
fn load(value: serde_json::Value) -> (BTreeMap<String, McpServerLaunch>, Vec<String>) {
    let mut warnings = Vec::new();
    let raw = Raw {
        key: "mcp.servers".to_owned(),
        value,
        origin: origin(),
    };
    let leaves = typed(vec![raw], Vec::new(), &mut warnings);
    let Some(Leaf::McpServers(servers)) = leaves.into_iter().next() else {
        panic!("no list");
    };
    let mut keys: Vec<String> = warnings
        .into_iter()
        .map(|warning| match warning.kind {
            WarningKind::InvalidValue { key, .. } | WarningKind::UnknownKey { key } => key,
            other => panic!("{other:?}"),
        })
        .collect();
    keys.sort();
    (servers, keys)
}

/// Remote entries, keychain refs and plain values load; an entry that breaks a rule is left
/// out with a warning naming it; a file's variable names are held only to what an OS takes.
#[test]
fn remote_entries_and_keychain_refs_type() {
    let command = abs_str("bin/s");
    let (servers, warnings) = load(json!({
        "local": {"command": command, "env": {"TOKEN": {"keychain": true}, "PLAIN": "v",
            "ProgramFiles(x86)": "C:\\x"}, "enabled": true},
        "remote": {"url": "https://mcp.example.com/mcp",
            "headers": {"Authorization": {"keychain": true}, "X-Plain": "p"}},
        "local-http": {"url": "http://127.0.0.1:3000/mcp"},
        "both": {"command": command, "url": "https://x.example", "headers": {}},
        "remote-args": {"url": "https://x.example", "args": ["a"]},
        "keychain-false": {"command": command, "env": {"A": {"keychain": false}}},
        "keychain-extra": {"command": command, "env": {"A": {"keychain": true, "x": 1}}},
        "http-remote": {"url": "http://example.com/mcp"},
        "bad-header": {"url": "https://x.example", "headers": {"Bad Header": "v"}},
        "header-twice": {"url": "https://x.example", "headers": {"A": "1", "a": "2"}},
        "env-equals": {"command": command, "env": {"A=B": "v"}},
        "neither": {"enabled": true},
    }));
    assert_eq!(
        servers.keys().map(String::as_str).collect::<Vec<_>>(),
        ["both", "local", "local-http", "remote", "remote-args"]
    );
    let saved = McpValue::Keychain(crate::schema::KeychainRef::default());
    assert_eq!(
        servers.get("local"),
        Some(&McpServerLaunch::local(
            command.clone(),
            Vec::new(),
            BTreeMap::from([
                ("PLAIN".to_owned(), McpValue::Plain("v".to_owned())),
                (
                    "ProgramFiles(x86)".to_owned(),
                    McpValue::Plain("C:\\x".to_owned())
                ),
                ("TOKEN".to_owned(), saved.clone()),
            ]),
            true,
        ))
    );
    assert_eq!(
        servers.get("remote").map(|server| &server.transport),
        Some(&McpTransport::Remote {
            url: "https://mcp.example.com/mcp".to_owned(),
            headers: BTreeMap::from([
                ("Authorization".to_owned(), saved),
                ("X-Plain".to_owned(), McpValue::Plain("p".to_owned())),
            ]),
        })
    );
    assert_eq!(
        servers.get("both").and_then(|s| s.command()),
        Some(command.as_str())
    );
    let expected: Vec<String> = [
        "bad-header",
        "both.headers",
        "both.url",
        "env-equals",
        "header-twice",
        "http-remote",
        "keychain-extra",
        "keychain-false",
        "neither",
        "remote-args.args",
    ]
    .iter()
    .map(|name| format!("mcp.servers.{name}"))
    .collect();
    assert_eq!(warnings, expected);
}

/// A server written by a build before remote servers (and before keychain refs) loads as it
/// did, and writes back the same JSON.
#[test]
fn an_old_entry_loads_and_writes_back_unchanged() {
    let entry = json!({"command": abs_str("bin/s"), "args": ["-v"], "env": {"A": "1"},
        "enabled": true});
    let (servers, warnings) = load(json!({ "old": &entry }));
    assert_eq!(warnings, Vec::<String>::new());
    let server = &servers["old"];
    assert_eq!(serde_json::to_value(server).unwrap(), entry);
    let remote = json!({"url": "https://x.example/mcp", "headers": {"A": {"keychain": true}},
        "enabled": false});
    let (servers, _) = load(json!({ "r": &remote }));
    assert_eq!(serde_json::to_value(&servers["r"]).unwrap(), remote);
}

/// A variable's value: an empty string is a plain value; `null`, a number, `{"keychain":
/// false}` or a ref with another key leaves the entry out with a warning.
#[test]
fn mcp_value_edge_cases() {
    let command = abs_str("bin/s");
    let (servers, warnings) = load(json!({
        "empty": {"command": command, "env": {"A": ""}},
        "null": {"command": command, "env": {"A": null}},
        "number": {"command": command, "env": {"A": 7}},
        "keychain-false": {"command": command, "env": {"A": {"keychain": false}}},
        "keychain-string": {"command": command, "env": {"A": {"keychain": "true"}}},
    }));
    assert_eq!(servers.keys().collect::<Vec<_>>(), ["empty"]);
    assert_eq!(
        servers["empty"].values().get("A"),
        Some(&McpValue::Plain(String::new()))
    );
    assert_eq!(
        warnings,
        ["keychain-false", "keychain-string", "null", "number"]
            .map(|name| format!("mcp.servers.{name}"))
    );
}

/// A program on a network share, which the program search refuses, is left out at load with a
/// warning naming why, not dropped silently at launch; a policy's too.
#[test]
fn a_network_command_is_left_out_with_a_warning() {
    let (servers, warnings) = load(json!({
        "unc": {"command": "\\\\server\\share\\tool.exe", "enabled": true},
        "slashes": {"command": "//server/share/tool", "enabled": true},
        "device": {"command": "\\\\?\\C:\\tool.exe", "enabled": true},
        "local": {"command": abs_str("bin/tool"), "enabled": true},
    }));
    assert_eq!(servers.keys().collect::<Vec<_>>(), ["local"]);
    assert_eq!(
        warnings,
        ["device", "slashes", "unc"].map(|name| format!("mcp.servers.{name}"))
    );
    assert!(!crate::mcp::command_ok("\\\\server\\share\\tool.exe"));
    assert!(crate::mcp::command_ok("npx"));
}

/// A `\\?\` path is refused for how it is written, not called a network share; a UNC path is.
#[test]
fn a_device_path_and_a_share_are_told_apart() {
    let mut warnings = Vec::new();
    let raw = Raw {
        key: "mcp.servers".to_owned(),
        value: json!({
            "device": {"command": r"\\?\C:\tool.exe"},
            "unc": {"command": r"\\server\share\tool.exe"},
        }),
        origin: origin(),
    };
    let _ = typed(vec![raw], Vec::new(), &mut warnings);
    let message = |name: &str| {
        warnings
            .iter()
            .find_map(|warning| match &warning.kind {
                WarningKind::InvalidValue { key, message }
                    if key == &format!("mcp.servers.{name}") =>
                {
                    Some(message.clone())
                }
                _ => None,
            })
            .unwrap_or_default()
    };
    assert!(
        message("device").contains(r"written with \\?\"),
        "{}",
        message("device")
    );
    assert!(!message("device").contains("network share"));
    assert!(
        message("unc").contains("network share"),
        "{}",
        message("unc")
    );
}

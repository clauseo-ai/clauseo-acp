//! A planted user settings file changes nothing a Clauseo launch pins: the single values lose to
//! the flag settings, and the merging ones are gone once the launch rewrites the file. Claude
//! Code's merge is modelled here ([`effective`]), as 2.1.293 does it (`S4`): arrays joined and
//! de-duplicated, objects merged key by key, a single value the higher source's.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::super::browser::Switch;
use super::super::profile::{ClaudeKeys, Folders, Login};
use super::super::sandbox::ClaudeSandbox;
use super::super::{McpControl, session_meta};
use super::security_pins;
use crate::agent::data_fence::LaunchDeny;

/// The settings Claude Code uses: `user` under `flag` (Claude Code's precedence, no policy).
fn effective(user: &Value, flag: &Value) -> Value {
    match (user, flag) {
        (Value::Object(low), Value::Object(high)) => {
            let mut out = low.clone();
            for (key, value) in high {
                let merged = match low.get(key) {
                    Some(under) => effective(under, value),
                    None => value.clone(),
                };
                out.insert(key.clone(), merged);
            }
            Value::Object(out)
        }
        (Value::Array(low), Value::Array(high)) => {
            let mut out: Vec<Value> = Vec::new();
            for item in low.iter().chain(high) {
                if !out.contains(item) {
                    out.push(item.clone());
                }
            }
            Value::Array(out)
        }
        (_, high) => high.clone(),
    }
}

/// The value at `path` in `value`.
fn at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |value, key| value.get(*key))
}

fn sandbox() -> ClaudeSandbox {
    ClaudeSandbox {
        allow_write: vec!["/matter".to_owned()],
        deny_write: vec!["/matter/.clauseo".to_owned()],
        deny_read: vec!["~/".to_owned()],
        allow_read: vec!["/matter".to_owned()],
        quiet: Vec::new(),
    }
}

/// The flag settings of a Claude launch with or without the sandbox, in forced ask or not.
fn flag_settings(sandboxed: bool, forced_ask: bool) -> Value {
    let sandbox = sandbox();
    let keys = ClaudeKeys {
        web_search: Switch {
            on: true,
            locked: false,
        },
        login: Login::FirstParty,
        folders: Folders {
            work: "/data/run/t-1/work",
            notes: "/data/agent-state/m/notes",
            connectors: "",
            browser: "",
        },
        seed_sites: &[],
        forced_ask,
        data: &LaunchDeny::default(),
        case_law: false,
        browser: false,
        sandbox: sandboxed.then_some(&sandbox),
        matter: None,
        trusted_servers: &[],
    };
    // A hook that tries to loosen every key, as a user file would.
    let hook =
        json!({"claudeCode": {"options": {"settings": opposite_of_all(sandboxed, forced_ask)}}});
    let meta = session_meta(
        hook.as_object().cloned(),
        McpControl::Clauseo,
        None,
        Some(&keys),
    );
    meta["claudeCode"]["options"]["settings"].clone()
}

/// The opposite of `value`: another bool, another mode, `null` for the bypass switch's one
/// value (a user file can only leave it out).
fn opposite(value: &Value) -> Value {
    match value {
        Value::Bool(on) => Value::Bool(!on),
        Value::String(text) if text == "disable" => Value::Null,
        Value::String(_) => Value::from("bypassPermissions"),
        Value::Null | Value::Number(_) | Value::Array(_) | Value::Object(_) => value.clone(),
    }
}

/// A settings object setting every pinned key to its opposite.
fn opposite_of_all(sandboxed: bool, forced_ask: bool) -> Value {
    let mut out = Value::Object(Map::new());
    for (path, value) in security_pins(sandboxed, forced_ask) {
        let mut slot = &mut out;
        for key in path {
            slot = slot
                .as_object_mut()
                .unwrap()
                .entry((*key).to_owned())
                .or_insert_with(|| json!({}));
        }
        *slot = opposite(&value);
    }
    out
}

/// For each pinned single value, a user file saying the opposite (each key alone, and all at
/// once) leaves the effective value the launch's, on every OS shape and mode.
#[test]
fn a_user_file_saying_the_opposite_changes_no_pinned_value() {
    for (sandboxed, forced_ask) in [(true, false), (true, true), (false, false), (false, true)] {
        let flag = flag_settings(sandboxed, forced_ask);
        let pins = security_pins(sandboxed, forced_ask);
        let all = opposite_of_all(sandboxed, forced_ask);
        for (path, value) in &pins {
            assert_eq!(
                at(&flag, path),
                Some(value),
                "{path:?} set in the flag settings"
            );
            let mut user = Value::Object(Map::new());
            let mut slot = &mut user;
            for key in *path {
                slot = slot
                    .as_object_mut()
                    .unwrap()
                    .entry((*key).to_owned())
                    .or_insert_with(|| json!({}));
            }
            *slot = opposite(value);
            for planted in [&user, &all] {
                let merged = effective(planted, &flag);
                assert_eq!(
                    at(&merged, path),
                    Some(value),
                    "{path:?} {sandboxed} {forced_ask}"
                );
            }
        }
    }
}

/// The values themselves: hooks never off, the mode the session's, bypass disabled, the sandbox
/// on with no way out where Clauseo runs it and off where it does not (Windows).
#[test]
fn the_pins_are_clauseos_values() {
    let flag = flag_settings(true, false);
    assert_eq!(flag["disableAllHooks"], json!(false));
    assert_eq!(flag["permissions"]["defaultMode"], json!("auto"));
    assert_eq!(
        flag["permissions"]["disableBypassPermissionsMode"],
        json!("disable")
    );
    assert_eq!(flag["sandbox"]["enabled"], json!(true));
    assert_eq!(flag["sandbox"]["autoAllowBashIfSandboxed"], json!(true));
    assert_eq!(flag["sandbox"]["allowUnsandboxedCommands"], json!(false));
    assert_eq!(flag["sandbox"]["failIfUnavailable"], json!(true));
    assert_eq!(
        flag["sandbox"]["network"]["allowLocalBinding"],
        json!(false)
    );
    // The sandbox's own lists stay beside the pins.
    assert_eq!(
        flag["sandbox"]["filesystem"]["allowWrite"],
        json!(["/matter"])
    );
    assert_eq!(flag["sandbox"]["network"]["allowedDomains"], json!(["*"]));
    let forced = flag_settings(true, true);
    assert_eq!(forced["permissions"]["defaultMode"], json!("default"));
    assert_eq!(forced["sandbox"]["autoAllowBashIfSandboxed"], json!(false));
    let windows = flag_settings(false, false);
    assert_eq!(windows["sandbox"]["enabled"], json!(false));
    assert_eq!(windows["sandbox"]["autoAllowBashIfSandboxed"], json!(false));
    assert_eq!(windows["sandbox"].get("filesystem"), None);
}

/// Every launch shape's flag settings turn the bundled skills off and deny three of them by name,
/// beside the hook's own deny rules; a user file saying `disableBundledSkills: false`
/// or holding no deny rules changes neither (a single value loses, arrays join).
#[test]
fn the_bundled_skills_are_off_and_the_loosening_ones_denied() {
    for (sandboxed, forced_ask) in [(true, false), (true, true), (false, false), (false, true)] {
        let flag = flag_settings(sandboxed, forced_ask);
        assert_eq!(flag["disableBundledSkills"], json!(true));
        let user = json!({"disableBundledSkills": false, "permissions": {"deny": []}});
        let merged = effective(&user, &flag);
        assert_eq!(merged["disableBundledSkills"], json!(true));
        let deny = merged["permissions"]["deny"].as_array().unwrap();
        for rule in super::SKILL_DENIES {
            assert_eq!(
                deny.iter()
                    .filter(|held| held.as_str() == Some(rule))
                    .count(),
                1,
                "{rule} once, {sandboxed} {forced_ask}"
            );
        }
    }
}

/// The merging keys and the `env` block, planted in the user file, are gone once the launch
/// rewrites it: the effective settings hold none of them.
#[test]
fn planted_merging_keys_are_gone_after_the_launch_writes_the_file() {
    let home = std::env::temp_dir().join(format!("clauseo-pinned-{}", std::process::id()));
    crate::write::remove_dir_all(&home).unwrap();
    crate::write::create_dir_all(&home).unwrap();
    let planted = json!({
        "permissions": {
            "allow": ["Bash(curl:*)"],
            // unix-paths-ok: a planted settings or env value; it never reaches a path API
            "additionalDirectories": ["/Users/lawyer"],
        },
        "sandbox": {
            // unix-paths-ok: a planted settings or env value; it never reaches a path API
            "filesystem": {"allowWrite": ["/Users/lawyer"]},
            "excludedCommands": ["curl"],
        },
        "env": {"IS_SANDBOX": "1", "CLAUDE_CODE_SANDBOXED": "1"},
        "hooks": {"PreToolUse": [{"matcher": "*", "hooks": []}]},
    });
    let file = home.join(crate::agent::home::CLAUDE_SETTINGS_FILE);
    crate::write::write_file(&file, planted.to_string().as_bytes()).unwrap();
    let flag = flag_settings(true, false);
    let before = effective(&planted, &flag);
    assert_eq!(
        before["permissions"]["allow"][0],
        json!("Bash(curl:*)"),
        "the model merges"
    );
    crate::agent::home::write_claude_settings(Path::new(&home)).unwrap();
    let left: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let after = effective(&left, &flag);
    let text = after.to_string();
    for value in [
        "Bash(curl:*)",
        // unix-paths-ok: a planted settings or env value; it never reaches a path API
        "/Users/lawyer",
        "\"curl\"",
        "IS_SANDBOX",
        "CLAUDE_CODE_SANDBOXED",
    ] {
        assert!(!text.contains(value), "{value} in {after}");
    }
    assert_eq!(at(&after, &["sandbox", "excludedCommands"]), None);
    assert_eq!(at(&after, &["hooks", "PreToolUse"]), None);
    crate::write::remove_dir_all(&home).unwrap();
}

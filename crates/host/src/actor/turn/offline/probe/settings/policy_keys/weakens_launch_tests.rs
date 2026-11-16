//! Clauseo's own launch values against a policy (`weakens::launch_pins`): its launch variables,
//! the launch's own `PATH` and `GIT_ATTR_SOURCE`, its flag settings and `tools`, in every
//! policy tier, at the launch and before each prompt (one check, `profile_loads/policy.rs`).

use std::collections::BTreeMap;
use std::ffi::OsString;

use serde_json::{Value, json};

use clauseo_protocol::PolicySetting;

use super::super::{AdminPolicy, Doc, Finding, REMOTE, USER_TIER, finding, user_policy_touches};
use super::{ENV_OWN, ENV_PINNED, OwnEnv, Weakening, launch_pins, launch_settings};
use crate::agent::loads::pinned::security_pins;

/// Every admin tier's source as `finding` names it, then the user tier's.
const SOURCES: [&str; 6] = [
    "managed-settings.json",
    "managed-settings.d",
    "macOS managed preferences",
    "HKLM\\SOFTWARE\\Policies\\ClaudeCode",
    REMOTE,
    USER_TIER,
];

/// A launch whose own `PATH` is `/clauseo/bin:/usr/bin` and that sets no `GIT_ATTR_SOURCE`.
fn own() -> OwnEnv {
    let env = BTreeMap::from([(
        OsString::from("PATH"),
        OsString::from("/clauseo/bin:/usr/bin"),
    )]);
    OwnEnv::of(&env)
}

/// What each tier finds in `policy` for a launch with the sandbox and `own`: one finding per
/// source, in [`SOURCES`]' order.
fn found(policy: &Value, own: Option<&OwnEnv>) -> Vec<Option<Finding>> {
    let doc = Doc::Settings(policy.as_object().cloned().unwrap());
    let mut pins = security_pins(true, false);
    pins.extend(launch_pins(own));
    let (admin, user) = SOURCES.split_at(5);
    let mut all: Vec<_> = admin
        .iter()
        .map(|source| finding(&[(*source, doc.clone())], &pins, true, false))
        .collect();
    all.extend(
        user.iter()
            .map(|_| user_policy_touches(AdminPolicy::of(&[]), &doc, &pins, true)),
    );
    all
}

/// Asserts every tier refuses `policy` on `key`, naming `setting` to the app.
fn refused_everywhere(
    policy: &Value,
    key: &'static [&'static str],
    setting: Option<PolicySetting>,
) {
    for (found, source) in found(policy, Some(&own())).into_iter().zip(SOURCES) {
        assert_eq!(
            found,
            Some(Finding::Weakens(Weakening { key, source })),
            "{policy} {source}"
        );
        assert_eq!(
            found.as_ref().and_then(Finding::policy_setting),
            setting,
            "{policy}"
        );
    }
}

/// Asserts no tier finds anything in `policy`.
fn passes_everywhere(policy: &Value) {
    for (found, source) in found(policy, Some(&own())).into_iter().zip(SOURCES) {
        assert_eq!(found, None, "{policy} {source}");
    }
}

#[test]
fn a_launch_variable_set_to_another_value_or_emptied_refuses_unnamed() {
    for (key, value) in ENV_PINNED {
        let name = key[1];
        let other = if value == "1" { "0" } else { "1" };
        for (spelled, bad) in [
            (name.to_owned(), json!(other)),
            (name.to_owned(), json!("")),
            (name.to_owned(), Value::Null),
            (name.to_owned(), json!(1)),
            (name.to_ascii_lowercase(), json!(other)),
        ] {
            refused_everywhere(&json!({"env": {spelled: bad}}), key, None);
        }
        // Clauseo's own value changes nothing.
        passes_everywhere(&json!({"env": {name: value}}));
    }
}

#[test]
fn a_policy_path_other_than_the_launch_path_refuses_and_is_named() {
    let path: &'static [&'static str] = ENV_OWN[0];
    for (spelled, bad) in [
        ("PATH", json!("/usr/bin")),
        ("PATH", json!("/clauseo/bin:/usr/bin:/firm/tools")),
        ("PATH", json!("")),
        ("Path", json!("/usr/bin")),
    ] {
        refused_everywhere(
            &json!({"env": {spelled: bad}}),
            path,
            Some(PolicySetting::EnvPath),
        );
    }
    passes_everywhere(&json!({"env": {"PATH": "/clauseo/bin:/usr/bin"}}));
}

#[test]
fn a_policy_git_attr_source_the_launch_does_not_set_refuses_unnamed() {
    let key: &'static [&'static str] = ENV_OWN[1];
    for bad in [json!("HEAD"), json!(""), Value::Null] {
        refused_everywhere(&json!({"env": {"GIT_ATTR_SOURCE": bad}}), key, None);
    }
}

#[test]
fn before_the_launch_environment_is_known_its_own_values_are_not_checked() {
    let policy = json!({"env": {"PATH": "/usr/bin", "GIT_ATTR_SOURCE": "HEAD"}});
    assert!(found(&policy, None).iter().all(Option::is_none));
}

#[test]
fn a_flag_setting_clauseo_sets_given_another_value_refuses_and_is_named() {
    for (key, ours) in launch_settings() {
        let setting = super::policy_setting(key);
        assert!(setting.is_some(), "{key:?}");
        let other = match &ours {
            Value::Bool(b) => json!(!b),
            _ => json!("allow"),
        };
        let mut policy = serde_json::Map::new();
        policy.insert(key[0].to_owned(), other);
        refused_everywhere(&Value::Object(policy), key, setting);
        let mut same = serde_json::Map::new();
        same.insert(key[0].to_owned(), ours);
        passes_everywhere(&Value::Object(same));
    }
}

#[test]
fn a_policy_tool_list_refuses_and_its_denied_tools_pass() {
    for tools in [json!(["Read"]), json!([]), json!("Read")] {
        refused_everywhere(
            &json!({"tools": tools}),
            &["tools"],
            Some(PolicySetting::Tools),
        );
    }
    passes_everywhere(&json!({"disallowedTools": ["WebFetch", "Bash(git:*)"]}));
}

#[test]
fn the_own_values_come_from_its_environment() {
    let env = BTreeMap::from([
        (OsString::from("Path"), OsString::from("C:\\clauseo")),
        (OsString::from("GIT_ATTR_SOURCE"), OsString::from("abc")),
    ]);
    let own = OwnEnv::of(&env);
    let pins = launch_pins(Some(&own));
    let pin = |key: &[&str]| pins.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
    // Windows reads `Path` as `PATH`; elsewhere the launch has no `PATH`, so any policy one
    // refuses (a `null` pin).
    let path = if cfg!(windows) {
        json!("C:\\clauseo")
    } else {
        Value::Null
    };
    assert_eq!(pin(ENV_OWN[0]), Some(path));
    assert_eq!(pin(ENV_OWN[1]), Some(json!("abc")));
    assert_eq!(
        launch_pins(None).len(),
        ENV_PINNED.len() + launch_settings().len()
    );
}

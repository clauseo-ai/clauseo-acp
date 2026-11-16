use serde_json::{Value, json};

use std::collections::BTreeSet;

use clauseo_protocol::PolicySetting;

use super::super::{AdminPolicy, Doc, Finding, REMOTE, USER_TIER, finding, user_policy_touches};
use super::{
    ANY_VALUE, ENV_REFUSED, HOOKS_OFF, Weakening, launch_settings, policy_setting, weakening,
};
use crate::agent::loads::pinned::security_pins;

fn settings(value: &Value) -> serde_json::Map<String, Value> {
    value.as_object().cloned().unwrap()
}

/// The first weakening key of `value` for a launch with the sandbox, out of forced ask.
fn sandboxed(value: &Value) -> Option<String> {
    let pins = security_pins(true, false);
    weakening(
        &settings(value),
        "managed-settings.json",
        &pins,
        true,
        false,
    )
    .map(|found| found.dotted())
}

/// The same for a launch with no sandbox (Windows), in forced ask or not.
fn unsandboxed(value: &Value, forced_ask: bool) -> Option<String> {
    let pins = security_pins(false, forced_ask);
    weakening(&settings(value), "HKLM", &pins, false, false).map(|found| found.dotted())
}

#[test]
fn every_weakening_value_is_named_by_its_key() {
    for (policy, key) in [
        (json!({"sandbox": {"enabled": false}}), "sandbox.enabled"),
        (
            json!({"sandbox": {"allowUnsandboxedCommands": true}}),
            "sandbox.allowUnsandboxedCommands",
        ),
        (
            json!({"sandbox": {"excludedCommands": ["docker"]}}),
            "sandbox.excludedCommands",
        ),
        (
            json!({"sandbox": {"failIfUnavailable": false}}),
            "sandbox.failIfUnavailable",
        ),
        (
            json!({"sandbox": {"enableWeakerNestedSandbox": true}}),
            "sandbox.enableWeakerNestedSandbox",
        ),
        (
            json!({"sandbox": {"network": {"allowLocalBinding": true}}}),
            "sandbox.network.allowLocalBinding",
        ),
        (
            json!({"sandbox": {"network": {"allowAllUnixSockets": true}}}),
            "sandbox.network.allowAllUnixSockets",
        ),
        (
            json!({"permissions": {"defaultMode": "acceptEdits"}}),
            "permissions.defaultMode",
        ),
        (
            json!({"permissions": {"defaultMode": "bypassPermissions"}}),
            "permissions.defaultMode",
        ),
        (
            json!({"permissions": {"defaultMode": "dontAsk"}}),
            "permissions.defaultMode",
        ),
        (
            json!({"permissions": {"disableBypassPermissionsMode": false}}),
            "permissions.disableBypassPermissionsMode",
        ),
        (
            json!({"disableBundledSkills": false}),
            "disableBundledSkills",
        ),
        (
            json!({"skipDangerousModePermissionPrompt": true}),
            "skipDangerousModePermissionPrompt",
        ),
        (
            json!({"allowManagedPermissionRulesOnly": true}),
            "allowManagedPermissionRulesOnly",
        ),
    ] {
        assert_eq!(sandboxed(&policy).as_deref(), Some(key), "{policy}");
    }
}

#[test]
fn a_value_that_only_tightens_passes() {
    for policy in [
        json!({"model": "opus", "env": {"X": "1"}}),
        json!({"sandbox": {"enabled": true, "failIfUnavailable": true, "excludedCommands": []}}),
        json!({"sandbox": {"autoAllowBashIfSandboxed": false, "allowAppleEvents": false}}),
        json!({"permissions": {"defaultMode": "default", "deny": ["Bash(curl:*)"]}}),
        json!({"permissions": {"defaultMode": "plan"}}),
        json!({"permissions": {"disableBypassPermissionsMode": "disable"}}),
        json!({"skipDangerousModePermissionPrompt": false}),
        json!({"allowManagedPermissionRulesOnly": false}),
        json!({"disableAllHooks": true, "hooks": {"PreToolUse": []}}),
    ] {
        assert_eq!(sandboxed(&policy), None, "{policy}");
    }
}

/// Without a sandbox (Windows) its keys change nothing, so a firm's policy written for every OS
/// passes; one that turns the sandbox on is read as it would run.
#[test]
fn the_sandbox_keys_count_only_with_a_sandbox() {
    let common = json!({"sandbox": {"enabled": false, "excludedCommands": ["docker"]}});
    assert_eq!(unsandboxed(&common, false), None);
    let on = json!({"sandbox": {"enabled": true, "allowUnsandboxedCommands": true}});
    assert_eq!(
        unsandboxed(&on, false).as_deref(),
        Some("sandbox.allowUnsandboxedCommands")
    );
    let auto = json!({"permissions": {"defaultMode": "auto"}});
    assert_eq!(unsandboxed(&auto, false), None, "auto is the pin");
    assert_eq!(
        unsandboxed(&auto, true).as_deref(),
        Some("permissions.defaultMode"),
        "auto is weaker than forced ask's default"
    );
}

#[test]
fn the_hooks_keys_count_only_when_asked_and_the_source_is_kept() {
    let pins = security_pins(false, false);
    let off = settings(&json!({"allowManagedHooksOnly": true}));
    assert_eq!(weakening(&off, "HKLM", &pins, false, false), None);
    assert_eq!(
        weakening(&off, "HKCU", &pins, false, true),
        Some(Weakening {
            key: &["allowManagedHooksOnly"],
            source: "HKCU",
        })
    );
}

/// Every key [`weakening`] can return: each launch's pins, the hooks keys and the three it
/// checks beside the pins.
fn every_key() -> BTreeSet<&'static [&'static str]> {
    let pins = [(true, false), (true, true), (false, false), (false, true)]
        .into_iter()
        .flat_map(|(sandboxed, forced_ask)| security_pins(sandboxed, forced_ask))
        .map(|(key, _)| key);
    let beside: [&'static [&'static str]; 3] = [
        &["sandbox", "excludedCommands"],
        &["skipDangerousModePermissionPrompt"],
        &["allowManagedPermissionRulesOnly"],
    ];
    let launch = launch_settings().map(|(key, _)| key);
    let named: [&'static [&'static str]; 1] = [&["env", "PATH"]];
    pins.chain(HOOKS_OFF)
        .chain(beside)
        .chain(launch)
        .chain(ANY_VALUE)
        .chain(named)
        .collect()
}

#[test]
fn every_key_the_check_returns_names_its_setting_by_its_dotted_key() {
    let keys = every_key();
    let mut named = BTreeSet::new();
    for key in &keys {
        let setting = policy_setting(key).unwrap_or_else(|| panic!("{key:?} names no setting"));
        let wire = serde_json::to_value(setting).unwrap();
        assert_eq!(wire, Value::from(key.join(".")), "{key:?}");
        named.insert(setting.variant_name());
    }
    // And every setting the view can name is one the check returns.
    let known = PolicySetting::ALL
        .iter()
        .filter(|s| **s != PolicySetting::Unknown);
    assert_eq!(named.len(), known.count());
    assert_eq!(named.len(), keys.len());
}

#[test]
fn a_key_off_the_list_names_no_setting() {
    for key in [
        &["somethingElse"][..],
        &["sandbox", "network", "httpProxyPort"],
        &["sandbox"],
        &["permissions", "allow"],
        &["sandbox", "enabled", "extra"],
        &[],
    ] {
        assert_eq!(policy_setting(key), None, "{key:?}");
    }
}

/// Every policy tier's source, as [`finding`] names it: the admin files and drop-ins, macOS
/// managed preferences, HKLM and the server-managed settings (the user tier, HKCU, apart).
const ADMIN_SOURCES: [&str; 5] = [
    "managed-settings.json",
    "managed-settings.d",
    "macOS managed preferences",
    "HKLM\\SOFTWARE\\Policies\\ClaudeCode",
    REMOTE,
];

#[test]
fn each_refused_variable_in_each_policy_tier_refuses_the_launch_unnamed() {
    for key in ENV_REFUSED {
        let name = key[1];
        for (spelled, value) in [(name.to_owned(), "0"), (name.to_ascii_lowercase(), "1")] {
            let doc = Doc::Settings(settings(
                &json!({"env": {"PATH_EXTRA": "x", spelled: value}}),
            ));
            for (sandboxed, forced_ask) in [(true, false), (false, false), (false, true)] {
                let pins = security_pins(sandboxed, forced_ask);
                for source in ADMIN_SOURCES {
                    let found = finding(&[(source, doc.clone())], &pins, sandboxed, false);
                    assert_eq!(
                        found,
                        Some(Finding::Weakens(Weakening { key, source })),
                        "{source}"
                    );
                    assert_eq!(
                        found.as_ref().and_then(Finding::policy_setting),
                        None,
                        "{key:?}"
                    );
                }
                let user = user_policy_touches(AdminPolicy::of(&[]), &doc, &pins, sandboxed);
                let source = USER_TIER;
                assert_eq!(
                    user,
                    Some(Finding::Weakens(Weakening { key, source })),
                    "{name}"
                );
            }
        }
    }
    // The rest of a policy's `env` stays the firm's.
    let other = settings(&json!({"env": {"HTTPS_PROXY": "http://proxy:8080"}}));
    assert_eq!(
        weakening(&other, "HKLM", &security_pins(false, false), false, false),
        None
    );
}

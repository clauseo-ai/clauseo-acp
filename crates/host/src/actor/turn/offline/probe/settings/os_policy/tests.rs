//! Claude Code's OS policy tiers (`os_policy.rs`): a managed-preferences plist and a registry
//! `Settings` value move the agent as `managed-settings.json` does. The readers take their
//! paths and values, so every test runs on every OS.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::AgentKind;
use plist::{Dictionary, Value};

use super::{TierRead, plist_moves, plist_paths_for, plists_moved, registry_moved};
use crate::actor::turn::offline::probe::settings::Moved;
use crate::actor::turn::offline::probe::{ReachClass, Target};
use crate::write;

/// A fresh folder for `test`.
fn folder(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "clauseo-probe-os-policy-{}-{test}",
        std::process::id()
    ));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

/// A plain Claude launch's target once a tier has (`moved`) or has not moved it.
fn target(moved: bool) -> Target {
    let env = [(OsString::from("PATH"), OsString::from("bin"))].into();
    Target::at_launch(AgentKind::Claude, &env, false)
        .unless_moved(moved.then_some(Moved::SettingsEnv))
}

/// A settings dictionary whose `env` holds `key`, beside a key that is no `env`.
fn settings(key: &str) -> Value {
    let env: Dictionary = [(key.to_owned(), Value::from("http://firm.example"))]
        .into_iter()
        .collect();
    let settings: Dictionary = [
        ("env".to_owned(), Value::Dictionary(env)),
        ("cleanupPeriodDays".to_owned(), Value::from(30)),
    ]
    .into_iter()
    .collect();
    Value::Dictionary(settings)
}

fn xml(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    value.to_writer_xml(&mut bytes).unwrap();
    bytes
}

fn binary(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    value.to_writer_binary(&mut bytes).unwrap();
    bytes
}

fn put(path: &Path, bytes: &[u8]) {
    write::write_file(path, bytes).unwrap();
}

/// A managed-preferences plist, XML or binary, whose `env` sets a base URL or a proxy leaves
/// the service unknown; an unrelated `env` key, a missing file, or an empty one does not.
#[test]
fn a_managed_preferences_plist_with_a_route_key_leaves_the_service_unknown() {
    let root = folder("plist");
    let (user, device) = (root.join("user.plist"), root.join("device.plist"));
    let paths = [user.clone(), device.clone()];
    assert!(!plists_moved(&paths), "absent");
    assert_eq!(target(plists_moved(&paths)), target(false));
    assert_ne!(target(false), Target::Unknown(ReachClass::UnknownService));

    for (key, bytes) in [
        ("ANTHROPIC_BASE_URL", xml(&settings("ANTHROPIC_BASE_URL"))),
        ("HTTPS_PROXY", binary(&settings("HTTPS_PROXY"))),
        ("https_proxy", xml(&settings("https_proxy"))),
    ] {
        put(&device, &bytes);
        assert!(plists_moved(&paths), "{key}");
        assert_eq!(
            target(plists_moved(&paths)),
            Target::Unknown(ReachClass::UnknownService),
            "{key}"
        );
    }
    write::remove_old_file(&device).unwrap();

    // The per-user file alone is read too.
    put(&user, &binary(&settings("CLAUDE_CODE_USE_BEDROCK")));
    assert!(plists_moved(&paths));

    for bytes in [
        xml(&settings("DISABLE_TELEMETRY")),
        binary(&settings("EDITOR")),
        Vec::new(),
    ] {
        put(&user, &bytes);
        assert!(!plists_moved(&paths), "{bytes:?}");
    }
    write::remove_dir_all(&root).unwrap();
}

/// A plist that is no property list, or whose `env` is no dictionary, moves the agent; one that
/// is no dictionary or has no `env` does not, as a `managed-settings.json` of the same shape.
#[test]
fn a_plist_that_does_not_parse_moves_and_one_without_env_does_not() {
    assert!(plist_moves(b"not a plist"));
    let env_text: Dictionary = [("env".to_owned(), Value::from("HTTPS_PROXY=x"))]
        .into_iter()
        .collect();
    assert!(plist_moves(&xml(&Value::Dictionary(env_text))));
    assert!(!plist_moves(&xml(&Value::Array(vec![Value::from("env")]))));
    let other: Dictionary = [("model".to_owned(), Value::from("opus"))]
        .into_iter()
        .collect();
    assert!(!plist_moves(&binary(&Value::Dictionary(other))));
}

/// A plist there but unreadable (a folder stands at its path) moves the agent.
#[test]
fn an_unreadable_plist_moves_the_agent() {
    let root = folder("unreadable");
    let path = root.join("com.anthropic.claudecode.plist");
    write::create_dir_all(&path).unwrap();
    assert!(plists_moved(&[path]));
    write::remove_dir_all(&root).unwrap();
}

/// Claude Code's file paths: the user's then the computer's under its domain; the computer's
/// alone when the user has no name or one that is no folder name.
#[test]
fn the_plist_paths_are_claude_codes() {
    let dir = Path::new("/Library/Managed Preferences");
    let device = dir.join("com.anthropic.claudecode.plist");
    let user = dir.join("lawyer").join("com.anthropic.claudecode.plist");
    let mut paths = plist_paths_for(Some("lawyer"));
    paths.sort();
    let mut expected = vec![device.clone(), user];
    expected.sort();
    assert_eq!(paths, expected);
    assert_eq!(plist_paths_for(None), vec![device.clone()]);
    assert_eq!(plist_paths_for(Some("..")), vec![device]);
}

/// A registry `Settings` value (HKLM's or HKCU's) whose JSON `env` sets a route key leaves the
/// service unknown; an unrelated key, an absent value or an empty one does not; one that cannot
/// be read, or is no JSON, does.
#[test]
fn a_registry_settings_value_with_a_route_key_leaves_the_service_unknown() {
    let set = |text: &str| -> TierRead { Ok(Some(text.to_owned())) };
    assert!(!registry_moved(&[Ok(None), Ok(None)]), "absent");
    assert!(!registry_moved(&[]), "not Windows");
    for (hklm, hkcu) in [
        (
            set(r#"{"env":{"ANTHROPIC_BASE_URL":"https://gw.firm.example"}}"#),
            Ok(None),
        ),
        (
            Ok(None),
            set("{\"env\": {\"HTTP_PROXY\": \"http://proxy:3128\"}}\r\n"),
        ),
    ] {
        let values = [hklm, hkcu];
        assert!(registry_moved(&values), "{values:?}");
        assert_eq!(
            target(registry_moved(&values)),
            Target::Unknown(ReachClass::UnknownService)
        );
    }
    for unrelated in [
        set(r#"{"env":{"DISABLE_TELEMETRY":"1"},"permissions":{"deny":[]}}"#),
        set(r#"{"model":"opus"}"#),
        set("  "),
    ] {
        let values = [unrelated, Ok(None)];
        assert!(!registry_moved(&values), "{values:?}");
        assert_eq!(target(registry_moved(&values)), target(false));
    }
    assert!(registry_moved(&[Err(()), Ok(None)]), "unreadable");
    assert!(registry_moved(&[set("{not json"), Ok(None)]), "no JSON");
}

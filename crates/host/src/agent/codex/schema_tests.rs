//! Every config Clauseo emits for Codex, checked against Codex 0.162.1's own config schema
//! (`tests/fixtures/codex/config.schema.json`, its README says where it comes from), with every
//! key outside the schema refused. Codex ignores unknown keys when it parses a config (a
//! misspelled key in a permission profile's `filesystem` table drops every deny,
//! `config/src/permissions_toml.rs:111-119` at `rust-v0.162.1`), so this is the check that a key
//! Clauseo writes is one Codex reads.
//!
//! The documents: the home `config.toml` ([`super::home_config`] then
//! [`super::side_profile_toml`], as [`super::write_home_config`] writes them, parsed as TOML);
//! a launch's `CODEX_CONFIG` ([`super::config_on`] with the profile's keys, the data-folder
//! fence and the guard's `hooks`), which codex-acp sends as each thread's `config` and, for
//! `hooks`, as `-c hooks=…`, so one top-level config document; and
//! `clauseo_limits::CODEX_OVERRIDES` folded into one document.
//!
//! The schema does not refuse every unknown key by itself, so [`schema`] tightens it on load:
//! - every object schema (one with `properties`, or of type `object`) whose
//!   `additionalProperties` is missing or `true` gets `additionalProperties: false`. Among them
//!   `HooksToml`, `MatcherGroup`, each `HookHandlerConfig` variant and `HookStateToml`, which
//!   carry no `additionalProperties`, and `PermissionsToml`, which is a bare `{"type": "object"}`;
//! - the two maps serde flattens on the permissions path, which schemars cannot describe, get
//!   their entries' schema back: `PermissionsToml` maps a profile name to
//!   `PermissionProfileToml` (`permissions_toml.rs:22-26`), and `FilesystemPermissionsToml` maps
//!   a path to `FilesystemPermissionToml` beside `glob_scan_max_depth`
//!   (`permissions_toml.rs:223-244`). Both entry definitions are in the schema, unreferenced.
//!
//! Other flattened maps (`WorkspaceRootsToml`, the network domain and socket tables) end up
//! closed: Clauseo writes none, and one written later fails here until it is described.

use std::collections::BTreeMap;
use std::path::Path;

use clauseo_settings::Locations;
use serde_json::{Value, json};

use crate::agent::codex_fence::tests::{codex_home, listed, notes, run_root};
use crate::agent::codex_fence::{Listed, PROFILE, SIDE_PROFILE, Session};

mod validator;

use validator::errors;

/// Codex 0.162.1's `codex-rs/core/config.schema.json`.
const SCHEMA: &str = include_str!("../../../tests/fixtures/codex/config.schema.json");

/// The flattened maps given their entries' schema back: (definition, entry definition).
const FLATTENED: [(&str, &str); 2] = [
    ("PermissionsToml", "PermissionProfileToml"),
    ("FilesystemPermissionsToml", "FilesystemPermissionToml"),
];

/// The `features` keys that turn Codex's hooks off when false: `hooks`, the key of
/// `Feature::CodexHooks` (`features/src/lib.rs:1296-1301`, default on), and its legacy alias
/// `codex_hooks` (`features/src/legacy.rs:48-51`). The session's hooks run only with it on
/// (`core/src/session/mod.rs:5159`).
const HOOK_FEATURES: [&str; 2] = ["hooks", "codex_hooks"];

/// A desktop program path for the guard's hook command.
const PROGRAM: &str = "/Applications/Clauseo.app/Contents/MacOS/clauseo-desktop";

/// The schema fixture as it is, untightened.
fn raw_schema() -> Value {
    serde_json::from_str(SCHEMA).expect("the schema fixture is JSON")
}

/// The schema with unknown keys refused everywhere (the module docs say how).
fn schema() -> Value {
    let mut root = raw_schema();
    // A regenerated schema that describes these maps itself is noticed here.
    for (name, _) in FLATTENED {
        let node = &root["definitions"][name];
        assert!(
            node.get("additionalProperties").is_none(),
            "{name} now sets additionalProperties: revisit the tightening"
        );
    }
    close_objects(&mut root);
    let definitions = root["definitions"]
        .as_object_mut()
        .expect("the schema has definitions");
    for (name, entry) in FLATTENED {
        assert!(definitions.contains_key(entry), "{entry} is defined");
        let node = definitions
            .get_mut(name)
            .and_then(Value::as_object_mut)
            .expect("the flattened map is defined");
        node.insert(
            "additionalProperties".to_owned(),
            json!({ "$ref": format!("#/definitions/{entry}") }),
        );
    }
    root
}

/// Sets `additionalProperties: false` on every object schema under `node` whose
/// `additionalProperties` is missing or `true`, following each place a subschema sits.
fn close_objects(node: &mut Value) {
    let Value::Object(map) = node else {
        return;
    };
    let is_object = map.contains_key("properties")
        || match map.get("type") {
            Some(Value::String(name)) => name == "object",
            Some(Value::Array(names)) => names.iter().any(|name| name == "object"),
            _ => false,
        };
    for key in ["additionalProperties", "items", "not"] {
        if let Some(sub) = map.get_mut(key) {
            close_objects(sub);
        }
    }
    for key in ["allOf", "anyOf", "oneOf"] {
        if let Some(Value::Array(subs)) = map.get_mut(key) {
            subs.iter_mut().for_each(close_objects);
        }
    }
    for key in ["properties", "definitions"] {
        if let Some(Value::Object(subs)) = map.get_mut(key) {
            subs.values_mut().for_each(close_objects);
        }
    }
    if is_object && !matches!(map.get("additionalProperties"), Some(Value::Object(_))) {
        map.insert("additionalProperties".to_owned(), Value::Bool(false));
    }
}

fn locations() -> Locations {
    // The data folder's root, as the fence's tests have it.
    Locations::under_root(Path::new(&crate::testing::abs("/home/lawyer")))
}

/// `text` parsed as TOML, as JSON.
fn toml_doc(text: &str) -> Value {
    toml::from_str(text).expect("the text is TOML")
}

/// The home `config.toml` as [`super::write_home_config`] writes it, off Windows and on it.
fn home_doc(windows: bool) -> Value {
    let side = super::side_profile_toml(&locations(), windows);
    assert_eq!(side.is_empty(), windows, "a side profile off Windows only");
    let mut text = super::home_config();
    text.push_str(&side);
    toml_doc(&text)
}

/// A launch's `CODEX_CONFIG`: the settings' model keys, the profile's keys (instructions, web
/// search, writable roots), the fence for `listed`, R15's pins, the guard's hooks, and the
/// shell keys of a session with folders on its `PATH` when `path_folders`.
fn launch_doc(windows: bool, listed: &Listed, path_folders: bool) -> Value {
    use crate::agent::loads::browser::Switch;
    use crate::agent::loads::profile::{Folders, codex_keys};

    let (run_root, notes, codex_home) = (run_root(), notes(), codex_home());
    let work = run_root.join("work");
    let Value::Object(hook) = json!({
        "developer_instructions": "Notes in {matter_notes}; work in {work_folder}.",
    }) else {
        panic!("the hook is an object")
    };
    let switch = Switch {
        on: true,
        locked: false,
    };
    let folders = Folders::of(&work, &notes).expect("UTF-8 folders");
    let mut launch = codex_keys(hook, switch, folders).expect("the profile's keys");
    let session = Session {
        run_root: &run_root,
        notes: &notes,
        codex_home: &codex_home,
        listed,
        socket_dir: Some(Path::new("/tmp/clauseo-ab-0123456789abcdef")),
    };
    let fence = crate::agent::codex_fence::keys(&locations(), session, windows);
    assert_eq!(fence.is_empty(), windows, "a fence off Windows only");
    launch.extend(fence);
    let record = run_root.join("guard").join("record.json");
    let hooks = crate::agent::loads::codex_hook_config(
        Path::new(PROGRAM),
        Some((&record, "0123456789abcdef")),
    )
    .and_then(|hooks| hooks.as_object().cloned())
    .expect("the guard's hooks");
    let settings = BTreeMap::from([(
        super::CONFIG_VAR.to_owned(),
        r#"{"model": "gpt-5.5", "model_reasoning_effort": "low"}"#.to_owned(),
    )]);
    let mut config = super::config_on(&settings, launch, Some(hooks), windows);
    if path_folders {
        super::without_login_shell(&mut config);
    }
    Value::Object(config)
}

/// `clauseo_limits::CODEX_OVERRIDES`, each `key=value` a TOML dotted key, in one document.
fn overrides_doc() -> Value {
    let mut text = String::new();
    for set in clauseo_limits::CODEX_OVERRIDES {
        let (key, value) = set.split_once('=').expect("each override is key=value");
        text.push_str(key);
        text.push_str(" = ");
        text.push_str(value);
        text.push('\n');
    }
    toml_doc(&text)
}

/// Every document Clauseo emits for Codex, named.
fn documents() -> Vec<(String, Value)> {
    let mut docs = vec![
        ("config.toml".to_owned(), home_doc(false)),
        ("config.toml on Windows".to_owned(), home_doc(true)),
        ("-c overrides".to_owned(), overrides_doc()),
    ];
    for (name, windows, listed, path_folders) in [
        ("CODEX_CONFIG", false, listed(), false),
        (
            "CODEX_CONFIG, nothing listed",
            false,
            Listed::default(),
            false,
        ),
        ("CODEX_CONFIG, PATH folders", false, listed(), true),
        ("CODEX_CONFIG on Windows", true, listed(), false),
    ] {
        docs.push((name.to_owned(), launch_doc(windows, &listed, path_folders)));
    }
    docs
}

#[test]
fn schema_accepts_every_config_clauseo_emits_for_codex() {
    let schema = schema();
    let mut failures = Vec::new();
    for (name, doc) in documents() {
        for error in errors(&schema, &doc) {
            failures.push(format!("{name}: {error}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The documents carry what the other tests look for, so a pass is not one over empty tables.
#[test]
fn schema_documents_hold_the_fence_the_side_profile_and_the_hooks() {
    let launch = launch_doc(false, &listed(), true);
    let filesystem = &launch["permissions"][PROFILE]["filesystem"];
    let table = filesystem.as_object().expect("a filesystem table");
    // Both entry shapes: a path's own access, and rules scoped under the data folder.
    assert!(table.values().any(Value::is_string));
    assert!(table.values().any(Value::is_object));
    assert_eq!(launch["default_permissions"], json!(PROFILE));
    assert!(launch["developer_instructions"].is_string());
    assert!(launch["web_search"].is_string());
    assert!(launch["sandbox_workspace_write"]["writable_roots"].is_array());
    assert!(launch["hooks"]["PreToolUse"][0]["hooks"][0]["command"].is_string());
    assert!(
        launch["hooks"]["state"]
            .as_object()
            .is_some_and(|state| !state.is_empty())
    );
    assert_eq!(launch["features"]["shell_snapshot"], json!(false));
    let home = home_doc(false);
    assert_eq!(home["default_permissions"], json!(SIDE_PROFILE));
    assert!(home["permissions"][SIDE_PROFILE]["filesystem"].is_object());
    assert_eq!(home["features"]["plugins"], json!(false));
    let windows = launch_doc(true, &listed(), false);
    assert_eq!(windows["windows"]["sandbox"], json!(super::WINDOWS_SANDBOX));
}

/// Each misspelling below is refused, naming the key, so the check is not vacuous.
#[test]
fn schema_refuses_a_misspelled_key() {
    let schema = schema();
    let launch = launch_doc(false, &listed(), false);
    let home = home_doc(false);
    let profile = format!("/permissions/{PROFILE}");
    let side = format!("/permissions/{SIDE_PROFILE}");
    // Each key with the letter at the index dropped, made at run time so the repository's
    // spell check reads no misspelling in the source.
    let cases: [(&Value, &str, &str, usize); 7] = [
        (&launch, profile.as_str(), "filesystem", 6),
        (&launch, profile.as_str(), "extends", 6),
        (&launch, "", "developer_instructions", 21),
        (&launch, "/sandbox_workspace_write", "writable_roots", 4),
        (&launch, "/hooks/PreToolUse/0/hooks/0", "timeout", 3),
        (&launch, "/hooks", "PreToolUse", 9),
        (&home, side.as_str(), "filesystem", 6),
    ];
    for (doc, parent, from, drop) in cases {
        let to: String = from
            .chars()
            .enumerate()
            .filter_map(|(index, letter)| (index != drop).then_some(letter))
            .collect();
        let mut doc = doc.clone();
        let table = doc
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("{parent} is a table"));
        let value = table
            .remove(from)
            .unwrap_or_else(|| panic!("{from} is set"));
        table.insert(to.clone(), value);
        let found = errors(&schema, &doc);
        assert!(
            found.iter().any(|error| error.contains(&format!("/{to}:"))),
            "{to} is refused: {found:?}"
        );
    }
    let mut features = launch.clone();
    features["features"]["hoks"] = json!(false);
    assert!(
        errors(&schema, &features)
            .iter()
            .any(|e| e.contains("hoks"))
    );
    let mut access = launch;
    let filesystem = access["permissions"][PROFILE]["filesystem"]
        .as_object_mut()
        .unwrap();
    let first = filesystem
        .values_mut()
        .find(|value| value.is_string())
        .unwrap();
    *first = json!("denied");
    assert!(
        !errors(&schema, &access).is_empty(),
        "an access mode outside the enum is refused"
    );
}

/// Why [`schema`] tightens: the fixture as it is lets a misspelled permission key through, as
/// Codex's parser does.
#[test]
fn schema_as_shipped_lets_a_misspelled_permission_key_through() {
    let mut doc = launch_doc(false, &listed(), false);
    let table = doc["permissions"][PROFILE].as_object_mut().unwrap();
    let filesystem = table.remove("filesystem").unwrap();
    // `filesystem` without its second `s`.
    table.insert(format!("{}tem", "filesystem".get(..6).unwrap()), filesystem);
    assert_eq!(errors(&raw_schema(), &doc), Vec::<String>::new());
    assert_ne!(errors(&schema(), &doc), Vec::<String>::new());
}

/// No document Clauseo emits turns Codex's hooks off: no [`HOOK_FEATURES`] key, and no hook
/// state with `enabled = false`.
#[test]
fn schema_no_config_clauseo_emits_turns_codex_hooks_off() {
    let features = &raw_schema()["properties"]["features"]["properties"];
    for key in HOOK_FEATURES {
        assert!(
            features.get(key).is_some(),
            "features.{key} is in Codex's schema"
        );
    }
    for (name, doc) in documents() {
        if let Some(set) = doc.get("features").and_then(Value::as_object) {
            for key in HOOK_FEATURES {
                assert!(!set.contains_key(key), "{name} sets features.{key}");
            }
        }
        let states = doc.pointer("/hooks/state").and_then(Value::as_object);
        for (source, state) in states.into_iter().flatten() {
            assert_ne!(
                state.get("enabled"),
                Some(&Value::Bool(false)),
                "{name} turns the hook {source} off"
            );
        }
    }
}

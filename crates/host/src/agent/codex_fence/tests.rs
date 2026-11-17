use std::path::{Path, PathBuf};

use clauseo_settings::Locations;
use serde_json::{Map, Value, json};

use super::{EXTENDS, PROFILE, Session, keys};

const ROOT: &str = "/home/lawyer";

fn locations() -> Locations {
    Locations::under_root(Path::new(ROOT))
}

fn data() -> PathBuf {
    locations().data_dir().to_path_buf()
}

fn run_root() -> PathBuf {
    data().join("run").join("s1")
}

fn work() -> PathBuf {
    data()
        .join("agent-state")
        .join("m1")
        .join("work")
        .join("s1")
}

fn notes() -> PathBuf {
    data().join("agent-state").join("m1").join("notes")
}

/// The profile for the session `s1` of matter `m1`, off Windows.
fn profile() -> Map<String, Value> {
    let (run_root, work, notes) = (run_root(), work(), notes());
    let session = Session {
        run_root: &run_root,
        work: &work,
        notes: &notes,
    };
    keys(&locations(), session, false)
}

/// Every mode value anywhere in the profile's `filesystem`, scoped ones included.
fn modes(keys: &Map<String, Value>) -> Vec<String> {
    fn walk(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(mode) => out.push(mode.clone()),
            Value::Object(map) => map.values().for_each(|value| walk(value, out)),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => {}
        }
    }
    let mut out = Vec::new();
    walk(&keys["permissions"][PROFILE]["filesystem"], &mut out);
    out
}

/// The profile has no `deny` entry on any OS: with one, Codex runs a command its reviewer
/// approved still sandboxed, without network (`codex-rs/core/src/tools/sandboxing.rs:252-296`).
#[test]
fn the_profile_has_no_deny_entry_on_any_os() {
    let keys = profile();
    assert_eq!(modes(&keys), ["write", "write", "write"], "{keys:?}");
    assert!(!Value::Object(keys).to_string().contains("deny"));
    let (run_root, work, notes) = (run_root(), work(), notes());
    let session = Session {
        run_root: &run_root,
        work: &work,
        notes: &notes,
    };
    let windows = keys_of(session, true);
    assert!(windows.is_empty());
}

fn keys_of(session: Session<'_>, windows: bool) -> Map<String, Value> {
    keys(&locations(), session, windows)
}

/// What a turn may write is unchanged: the profile extends `:workspace` and names the session's
/// run, work and notes folders `write`, exactly, scoped under the data folder, as before.
#[test]
fn the_profile_names_the_own_run_work_and_notes_folders_write_and_nothing_else() {
    let keys = profile();
    assert_eq!(keys["default_permissions"], json!(PROFILE));
    assert_eq!(keys["permissions"][PROFILE]["extends"], json!(EXTENDS));
    assert_eq!(EXTENDS, ":workspace");
    let filesystem = keys["permissions"][PROFILE]["filesystem"]
        .as_object()
        .unwrap();
    assert_eq!(filesystem.len(), 1, "{filesystem:?}");
    let scoped = &filesystem[data().to_str().unwrap()];
    assert_eq!(
        scoped,
        &json!({
            "run/s1": "write",
            "agent-state/m1/work/s1": "write",
            "agent-state/m1/notes": "write",
        })
    );
}

/// A folder outside the data folder is keyed by its own path.
#[test]
fn a_folder_outside_the_data_folder_is_keyed_by_its_own_path() {
    let outside = data().parent().unwrap().join("elsewhere");
    let (run_root, work, notes) = (run_root(), work(), outside);
    let session = Session {
        run_root: &run_root,
        work: &work,
        notes: &notes,
    };
    let keys = keys_of(session, false);
    let filesystem = &keys["permissions"][PROFILE]["filesystem"];
    assert_eq!(filesystem[notes.to_str().unwrap()], json!("write"));
    assert_eq!(
        filesystem[data().to_str().unwrap()],
        json!({"run/s1": "write", "agent-state/m1/work/s1": "write"})
    );
}

#[test]
fn the_profile_reaches_codex_config_whole_and_the_hook_cannot_set_it() {
    use crate::agent::loads::browser::Switch;
    use crate::agent::loads::profile::{Folders, codex_keys};

    let Value::Object(hook) = json!({
        "developer_instructions": "Work.",
        "default_permissions": ":danger-no-sandbox",
        "permissions": {"open": {"filesystem": {":root": "write"}}},
    }) else {
        panic!("the hook is an object")
    };
    let work = run_root().join("work");
    let notes = notes();
    let folders = Folders::of(&work, &notes).unwrap();
    let switch = Switch {
        on: false,
        locked: false,
    };
    let mut launch = codex_keys(hook, switch, folders).unwrap();
    assert_eq!(launch.get("default_permissions"), None);
    assert_eq!(launch.get("permissions"), None);
    let profile = profile();
    launch.extend(profile.clone());
    let config =
        crate::agent::codex::config_on(&std::collections::BTreeMap::new(), launch, None, false);
    assert_eq!(
        config["default_permissions"],
        profile["default_permissions"]
    );
    assert_eq!(config["permissions"], profile["permissions"]);
    assert_eq!(config.get("sandbox_mode"), None);
}

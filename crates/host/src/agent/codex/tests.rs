use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, MatterId};
use serde_json::{Map, Value, json};

use super::{
    config, config_on, guard_trusted, home_config, launch_env, make_home, merge_add_only,
    prepare_launch, refuse_codex_folder, write_home_config,
};
use crate::write;

/// A fresh folder under the temp folder for `test`, removed first.
fn scratch(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("clauseo-codex-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

/// The JSON object of each of `clauseo_limits::CODEX_OVERRIDES`, written out: a key the limits
/// crate adds fails here until the nesting is checked.
fn switches() -> Value {
    json!({
        "features": {"apps": false, "plugins": false, "skill_mcp_dependency_install": false},
        "cloud": {"skills": {"enabled": false}},
        "skills": {"bundled": {"enabled": false}, "include_instructions": false},
    })
}

/// What [`config`] gives with nothing else on this platform: the switches, and on Windows the
/// unelevated sandbox.
fn pinned() -> Value {
    let mut pinned = switches();
    if cfg!(windows) {
        pinned["windows"] = json!({"sandbox": "unelevated"});
    }
    pinned
}

fn object(value: Value) -> Map<String, Value> {
    let Value::Object(object) = value else {
        panic!("an object");
    };
    object
}

/// A dotted key goes in only when nothing of its first part is there: it never lands beside a
/// nested key codex would read it over.
#[test]
fn merge_add_only_takes_a_dotted_key_only_where_nothing_clashes() {
    let mut held = object(json!({
        "sandbox_workspace_write": {"writable_roots": ["/work"]},
        "a.b": 1,
    }));
    merge_add_only(
        &mut held,
        object(json!({
            "sandbox_workspace_write.writable_roots": ["/elsewhere"],
            "a.b": 2,
            "fresh.key": true,
        })),
    );
    assert_eq!(
        Value::Object(held),
        json!({
            "sandbox_workspace_write": {"writable_roots": ["/work"]},
            "a.b": 1,
            "fresh.key": true,
        })
    );
}

/// Arrays on both sides gain only the items they lack, in order; a value of another type
/// already there stays, and objects merge key by key.
#[test]
fn merge_add_only_extends_arrays_and_keeps_what_is_there() {
    let mut held = object(json!({
        "roots": ["/a", "/b"],
        "mode": "on-request",
        "nested": {"x": 1, "list": [1]},
    }));
    merge_add_only(
        &mut held,
        object(json!({
            "roots": ["/b", "/c", "/a", "/d"],
            "mode": ["never"],
            "nested": {"x": 2, "y": 3, "list": [1, 2]},
            "new": [1],
        })),
    );
    assert_eq!(
        Value::Object(held),
        json!({
            "roots": ["/a", "/b", "/c", "/d"],
            "mode": "on-request",
            "nested": {"x": 1, "list": [1, 2], "y": 3},
            "new": [1],
        })
    );
}

#[test]
fn config_nests_every_limits_override() {
    let built = Value::Object(config(&BTreeMap::new(), Map::new(), None));
    assert_eq!(built, pinned());
    assert_eq!(clauseo_limits::CODEX_OVERRIDES.len(), 6);
}

/// On Windows Codex gets the unelevated sandbox, without which codex 0.162.1 makes
/// `workspace-write` read-only there and asks before every command; elsewhere there is no
/// `windows` key. A settings or profile `windows` key changes neither.
#[test]
fn only_windows_gets_the_unelevated_sandbox() {
    let mut on_windows = switches();
    on_windows["windows"] = json!({"sandbox": "unelevated"});
    let none = BTreeMap::new();
    assert_eq!(
        Value::Object(config_on(&none, Map::new(), None, true)),
        on_windows
    );
    assert_eq!(
        Value::Object(config_on(&none, Map::new(), None, false)),
        switches()
    );
    let elevated = json!({"windows": {"sandbox": "elevated"}});
    let settings = BTreeMap::from([("CODEX_CONFIG".to_owned(), elevated.to_string())]);
    for windows in [true, false] {
        let built = config_on(&settings, object(elevated.clone()), None, windows);
        let expected = if windows { &on_windows } else { &switches() };
        assert_eq!(&Value::Object(built), expected, "windows: {windows}");
    }
    assert_eq!(
        config(&none, Map::new(), None),
        config_on(&none, Map::new(), None, cfg!(windows))
    );
}

/// A settings `CODEX_CONFIG` gives only the model and its effort; it cannot turn a switch back
/// on, set hooks, or trust a project.
#[test]
fn settings_config_gives_only_the_model() {
    let settings = BTreeMap::from([(
        "codex_config".to_owned(),
        json!({
            "model": "gpt-6-luna",
            "model_reasoning_effort": "low",
            "features": {"apps": true, "plugins": true},
            "skills": {"bundled": {"enabled": true}},
            "hooks": {"PreToolUse": []},
            "projects": {"/": {"trust_level": "trusted"}},
        })
        .to_string(),
    )]);
    let mut expected = pinned();
    expected["model"] = json!("gpt-6-luna");
    expected["model_reasoning_effort"] = json!("low");
    assert_eq!(Value::Object(config(&settings, Map::new(), None)), expected);
    // Not JSON, or a model that is not a string: nothing is taken.
    for text in ["model = 'x'", r#"{"model": 5}"#, "[]"] {
        let settings = BTreeMap::from([("CODEX_CONFIG".to_owned(), text.to_owned())]);
        assert_eq!(
            Value::Object(config(&settings, Map::new(), None)),
            pinned(),
            "{text}"
        );
    }
}

/// The hooks seam's object goes in as `hooks`, whole: codex-acp makes it the one `-c hooks=…`.
#[test]
fn hooks_go_in_whole() {
    let hooks = json!({
        "PreToolUse": [{"matcher": "apply_patch", "hooks": [{"type": "command", "command": "/g"}]}],
        "state": {"/<session-flags>/config.toml:pre_tool_use:0:0": {"trusted_hash": "sha256:00"}},
    });
    let Value::Object(object) = hooks.clone() else {
        panic!("an object");
    };
    let built = config(&BTreeMap::new(), Map::new(), Some(object));
    assert_eq!(built.get("hooks"), Some(&hooks));
}

/// Every variable the host forces on a Codex agent, whatever the settings say.
#[test]
fn launch_env_forces_mode_home_config_and_no_filtering() {
    let home = Path::new("/data/agents/codex-acp/home");
    let settings = BTreeMap::from([
        (
            "INITIAL_AGENT_MODE".to_owned(),
            "agent-full-access".to_owned(),
        ),
        ("CODEX_HOME".to_owned(), "/Users/ada/.codex".to_owned()),
        (
            "DISABLE_MCP_CONFIG_FILTERING".to_owned(),
            "false".to_owned(),
        ),
    ]);
    let env = launch_env(home, &settings, Map::new(), None);
    let get = |name: &str| env.get(&OsString::from(name)).cloned();
    assert_eq!(get("INITIAL_AGENT_MODE"), Some("workspace-write".into()));
    assert_eq!(get("CODEX_HOME"), Some(home.as_os_str().to_owned()));
    assert_eq!(get("DISABLE_MCP_CONFIG_FILTERING"), Some("true".into()));
    let text = get("CODEX_CONFIG").unwrap();
    let parsed: Value = serde_json::from_str(text.to_str().unwrap()).unwrap();
    assert_eq!(parsed, pinned());
    assert_eq!(env.len(), 4);
}

#[test]
fn the_guard_is_trusted_only_when_every_listed_hook_is() {
    let trusted = json!({"key": "k", "currentHash": "sha256:00", "trustStatus": "trusted"});
    let untrusted = json!({"key": "k2", "currentHash": "sha256:01", "trustStatus": "untrusted"});
    assert!(guard_trusted(&json!({"cwd": "/m", "hooks": [trusted]})));
    assert!(!guard_trusted(&json!({"hooks": [trusted, untrusted]})));
    assert!(!guard_trusted(&json!({"hooks": [untrusted]})));
    // No hook listed: the guard is not there at all.
    assert!(!guard_trusted(&json!({"hooks": []})));
    assert!(!guard_trusted(&json!({})));
    assert!(!guard_trusted(&json!({"hooks": [{"key": "k"}]})));
}

#[test]
fn a_codex_entry_of_any_kind_refuses_the_folder() {
    let root = scratch("refuse");
    let clean = root.join("clean");
    write::create_dir_all(&clean).unwrap();
    assert!(refuse_codex_folder(&clean).is_ok());

    let with_folder = root.join("folder");
    write::create_dir_all(&with_folder.join(".codex")).unwrap();
    let error = refuse_codex_folder(&with_folder).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterCodexFolder);
    assert!(error.to_string().contains("Rename or remove"), "{error}");
    // No path in the words the lawyer may see.
    assert!(
        !error.to_string().contains(&*root.to_string_lossy()),
        "{error}"
    );

    let with_file = root.join("file");
    write::create_dir_all(&with_file).unwrap();
    write::write_file(&with_file.join(".codex"), b"").unwrap();
    assert!(refuse_codex_folder(&with_file).is_err());
    write::remove_dir_all(&root).unwrap();
}

/// A `.codex` stat that fails for another reason than not-found (here a folder the user may
/// not search) fails closed as `matter.folder_missing`, not as a `.codex` to rename. Skipped
/// as root, who may search it anyway.
#[cfg(unix)]
#[test]
fn a_stat_that_fails_fails_closed_as_folder_missing() {
    let root = scratch("refuse-locked");
    let locked = root.join("locked");
    write::create_dir_all(&locked).unwrap();
    write::set_mode(&locked, 0o000).unwrap();
    let as_root = std::fs::read_dir(&locked).is_ok();
    let refused = refuse_codex_folder(&locked);
    write::set_mode(&locked, 0o700).unwrap();
    write::remove_dir_all(&root).unwrap();
    if as_root {
        return;
    }
    assert_eq!(refused.unwrap_err().code(), ErrorCode::MatterFolderMissing);
}

/// A matter stored as a link resolves to the folder it names, and that folder's `.codex` counts.
#[cfg(unix)]
#[test]
fn a_linked_matter_is_checked_where_it_points() {
    let root = scratch("linked");
    let real = root.join("real");
    write::create_dir_all(&real.join(".codex")).unwrap();
    let link = root.join("link");
    write::symlink(&real, &link).unwrap();
    let resolved = crate::agent::session_folder(&link);
    assert_eq!(resolved, dunce::canonicalize(&real).unwrap());
    assert!(refuse_codex_folder(&resolved).is_err());
    // A path that does not resolve stays as stored.
    let gone = root.join("gone");
    assert_eq!(crate::agent::session_folder(&gone), gone);
    write::remove_dir_all(&root).unwrap();
}

/// The home under `data`, as `Locations::agent_home` makes it.
fn home_in(data: &Path) -> PathBuf {
    data.join("agents").join("codex-acp").join("home")
}

#[test]
fn make_home_makes_a_private_folder() {
    let root = scratch("home");
    let home = home_in(&root);
    make_home(&root, &home).unwrap();
    assert!(home.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for folder in [
            root.join("agents"),
            root.join("agents/codex-acp"),
            home.clone(),
        ] {
            let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}", folder.display());
        }
    }
    // Again: an existing home is fine.
    make_home(&root, &home).unwrap();
    // A file where the home goes is refused, with no path in its words.
    let other = scratch("home-file");
    write::create_dir_all(&other.join("agents/codex-acp")).unwrap();
    write::write_file(&home_in(&other), b"").unwrap();
    let error = make_home(&other, &home_in(&other)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    assert!(
        error
            .to_string()
            .ends_with("Clauseo couldn't prepare Codex's folder."),
        "{error}"
    );
    assert!(
        !error.to_string().contains(&*other.to_string_lossy()),
        "{error}"
    );
    write::remove_dir_all(&root).unwrap();
    write::remove_dir_all(&other).unwrap();
}

/// Two launches making the same home at once on a fresh data folder both succeed: one's
/// `AlreadyExists` is not a failure.
#[test]
fn concurrent_make_home_calls_both_succeed() {
    for round in 0..40 {
        let root = scratch(&format!("home-race-{round}"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let racers: Vec<_> = (0..2)
            .map(|_| {
                let (root, barrier) = (root.clone(), std::sync::Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    make_home(&root, &home_in(&root))
                })
            })
            .collect();
        for racer in racers {
            racer.join().unwrap().unwrap();
        }
        assert!(home_in(&root).is_dir());
        write::remove_dir_all(&root).unwrap();
    }
}

/// A link at `agents` or at `agents/<id>` is refused, and nothing is made where it points.
#[cfg(unix)]
#[test]
fn make_home_refuses_a_link_below_the_data_folder() {
    for linked in ["agents", "agents/codex-acp"] {
        let root = scratch("home-link");
        let data = root.join("data");
        let elsewhere = root.join("elsewhere");
        write::create_dir_all(&elsewhere).unwrap();
        let at = data.join(linked);
        write::create_dir_all(at.parent().unwrap()).unwrap();
        write::symlink(&elsewhere, &at).unwrap();
        let error = make_home(&data, &home_in(&data)).unwrap_err();
        assert_eq!(error.code(), ErrorCode::AgentLaunchFailed, "{linked}");
        assert_eq!(
            std::fs::read_dir(&elsewhere).unwrap().count(),
            0,
            "{linked}"
        );
        write::remove_dir_all(&root).unwrap();
    }
}

/// The data folder itself may be a link: the home is made inside the folder it names.
#[cfg(unix)]
#[test]
fn make_home_follows_a_linked_data_folder() {
    let root = scratch("home-data-link");
    let real = root.join("real");
    write::create_dir_all(&real).unwrap();
    let data = root.join("data");
    write::symlink(&real, &data).unwrap();
    make_home(&data, &home_in(&data)).unwrap();
    assert!(home_in(&real).is_dir());
    write::remove_dir_all(&root).unwrap();
}

/// The whole preparation: a `.codex` refuses before the home is made; otherwise it makes the
/// home and its `config.toml` and gives the folder resolved through links.
#[test]
fn prepare_launch_checks_then_makes_the_home() {
    let root = scratch("prepare");
    let data = root.join("data");
    write::create_dir_all(&data).unwrap();
    let matter = root.join("matter");
    write::create_dir_all(&matter.join(".codex")).unwrap();
    let id = MatterId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0001,
    ))
    .unwrap();
    let check = crate::agent::folder_support::FolderCheck {
        matter: id,
        source: crate::agent::folder_support::CheckSource::Windows,
    };
    let error = prepare_launch(&check, &matter, &data, &home_in(&data)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterCodexFolder);
    assert!(!data.join("agents").exists());
    write::remove_dir_all(&matter.join(".codex")).unwrap();
    let (folder, support) = prepare_launch(&check, &matter, &data, &home_in(&data)).unwrap();
    assert_eq!(folder, dunce::canonicalize(&matter).unwrap());
    // A folder the test made is fine on Windows, and off Windows nothing is checked.
    assert_eq!(support, crate::agent::folder_support::Answer::Known(None));
    assert!(home_in(&data).join("config.toml").is_file());
    write::remove_dir_all(&root).unwrap();
}

/// The home's `config.toml`: the header, then each override as a TOML dotted key, and nothing
/// else (no hooks table, whatever the seam gives: hooks go only through `CODEX_CONFIG`).
#[test]
fn home_config_holds_only_the_switches() {
    let text = home_config();
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("# Clauseo manages this file."),
        "{text}"
    );
    assert!(lines[1].starts_with("# "), "{text}");
    assert_eq!(
        lines[2..],
        [
            "features.apps = false",
            "features.plugins = false",
            "features.skill_mcp_dependency_install = false",
            "cloud.skills.enabled = false",
            "skills.bundled.enabled = false",
            "skills.include_instructions = false",
        ]
    );
    assert!(!text.contains("hooks"), "{text}");
}

/// Written at 0600, written again over an edit, and `auth.json` beside it untouched.
#[test]
fn home_config_is_rewritten_and_auth_json_is_left_alone() {
    let root = scratch("home-config");
    let home = home_in(&root);
    make_home(&root, &home).unwrap();
    let auth = br#"{"tokens": "not-a-real-token"}"#;
    write::write_file(&home.join("auth.json"), auth).unwrap();
    write_home_config(&home).unwrap();
    let path = home.join("config.toml");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), home_config());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    write::write_file(&path, b"features.plugins = true\n").unwrap();
    write_home_config(&home).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), home_config());
    assert_eq!(std::fs::read(home.join("auth.json")).unwrap(), auth);
    // No temp file is left beside it.
    let mut names: Vec<String> = std::fs::read_dir(&home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["auth.json", "config.toml"]);
    write::remove_dir_all(&root).unwrap();
}

/// The same bytes already there, as a plain 0600 file: no write at all (the file is the same
/// one). Any other content, or a link in its place, is written over.
#[cfg(unix)]
#[test]
fn an_identical_home_config_is_not_written_again() {
    use std::os::unix::fs::MetadataExt;
    let root = scratch("home-config-same");
    let home = home_in(&root);
    make_home(&root, &home).unwrap();
    write_home_config(&home).unwrap();
    let path = home.join("config.toml");
    let first = std::fs::symlink_metadata(&path).unwrap().ino();
    write_home_config(&home).unwrap();
    assert_eq!(std::fs::symlink_metadata(&path).unwrap().ino(), first);
    // A link to a file holding the same bytes is replaced by a file, never followed.
    let elsewhere = root.join("elsewhere.toml");
    write::write_file(&elsewhere, home_config().as_bytes()).unwrap();
    write::remove_file(&path).unwrap();
    write::symlink(&elsewhere, &path).unwrap();
    write_home_config(&home).unwrap();
    assert!(std::fs::symlink_metadata(&path).unwrap().is_file());
    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), home_config());
    write::remove_dir_all(&root).unwrap();
}

/// Before a write, temp files a crashed write left beside `config.toml` are removed; other
/// files stay.
#[test]
fn stale_temp_files_are_swept_before_a_write() {
    let root = scratch("home-config-sweep");
    let home = home_in(&root);
    make_home(&root, &home).unwrap();
    for stale in [
        "config.toml.clauseo-1-0.new",
        "config.toml.clauseo-99999-7.new",
    ] {
        write::write_file(&home.join(stale), b"half").unwrap();
    }
    write::write_file(&home.join("auth.json"), b"{}").unwrap();
    write::write_file(&home.join("other.clauseo-1-0.new"), b"x").unwrap();
    write_home_config(&home).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["auth.json", "config.toml", "other.clauseo-1-0.new"]);
    write::remove_dir_all(&root).unwrap();
}

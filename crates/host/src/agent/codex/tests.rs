use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;
use serde_json::{Value, json};

use super::{config, guard_trusted, launch_env, make_home, refuse_codex_folder, session_folder};
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
        "skills": {"bundled": {"enabled": false}},
    })
}

#[test]
fn config_nests_every_limits_override() {
    let built = Value::Object(config(&BTreeMap::new(), None));
    assert_eq!(built, switches());
    assert_eq!(clauseo_limits::CODEX_OVERRIDES.len(), 5);
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
    let mut expected = switches();
    expected["model"] = json!("gpt-6-luna");
    expected["model_reasoning_effort"] = json!("low");
    assert_eq!(Value::Object(config(&settings, None)), expected);
    // Not JSON, or a model that is not a string: nothing is taken.
    for text in ["model = 'x'", r#"{"model": 5}"#, "[]"] {
        let settings = BTreeMap::from([("CODEX_CONFIG".to_owned(), text.to_owned())]);
        assert_eq!(Value::Object(config(&settings, None)), switches(), "{text}");
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
    let built = config(&BTreeMap::new(), Some(object));
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
    let env = launch_env(home, &settings, None);
    let get = |name: &str| env.get(&OsString::from(name)).cloned();
    assert_eq!(get("INITIAL_AGENT_MODE"), Some("workspace-write".into()));
    assert_eq!(get("CODEX_HOME"), Some(home.as_os_str().to_owned()));
    assert_eq!(get("DISABLE_MCP_CONFIG_FILTERING"), Some("true".into()));
    let text = get("CODEX_CONFIG").unwrap();
    let parsed: Value = serde_json::from_str(text.to_str().unwrap()).unwrap();
    assert_eq!(parsed, switches());
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
    assert_eq!(error.code(), ErrorCode::MatterFolderRefused);
    assert!(error.to_string().contains("Rename or remove"), "{error}");

    let with_file = root.join("file");
    write::create_dir_all(&with_file).unwrap();
    write::write_file(&with_file.join(".codex"), b"").unwrap();
    assert!(refuse_codex_folder(&with_file).is_err());
    write::remove_dir_all(&root).unwrap();
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
    let resolved = session_folder(&link);
    assert_eq!(resolved, dunce::canonicalize(&real).unwrap());
    assert!(refuse_codex_folder(&resolved).is_err());
    // A path that does not resolve stays as stored.
    let gone = root.join("gone");
    assert_eq!(session_folder(&gone), gone);
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn make_home_makes_a_private_folder() {
    let root = scratch("home");
    let home = root.join("agents").join("codex-acp").join("home");
    make_home(&home).unwrap();
    assert!(home.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&home).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
    // Again: an existing home is fine.
    make_home(&home).unwrap();
    // A file where the home goes is refused.
    let file = root.join("file-home");
    write::write_file(&file, b"").unwrap();
    assert!(make_home(&file).is_err());
    write::remove_dir_all(&root).unwrap();
}

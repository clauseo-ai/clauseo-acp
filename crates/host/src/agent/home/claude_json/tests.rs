use std::path::{Path, PathBuf};

use super::{CLAUDE_JSON, CONFIG_KEYS, strip_config};
use crate::write;

/// A fresh home under the temp folder, named for `test`.
fn fresh(test: &str) -> PathBuf {
    let home =
        std::env::temp_dir().join(format!("clauseo-claude-json-{test}-{}", std::process::id()));
    write::remove_dir_all(&home).unwrap();
    write::create_dir_all(&home).unwrap();
    home
}

/// The JSON object in `<home>/.claude.json`.
fn read(home: &Path) -> serde_json::Map<String, serde_json::Value> {
    let bytes = std::fs::read(home.join(CLAUDE_JSON)).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// State as a real home holds it: the account, ids, caches, usage counts, migrations and a
/// Console key's approval, with a key Claude Code may add later.
fn state() -> serde_json::Value {
    serde_json::json!({
        "oauthAccount": {"accountUuid": "a", "emailAddress": "lawyer@example.com"},
        "userID": "u",
        "machineID": "m",
        "numStartups": 3,
        "cachedGrowthBookFeatures": {"x": true},
        "additionalModelCostsCache": {},
        "pluginUsage": {},
        "skillUsage": {"legal-drafting": 2},
        "seenNotifications": ["n"],
        "migrationVersion": 7,
        "customApiKeyResponses": {"approved": ["k"], "rejected": []},
        "primaryApiKey": "sk-test",
        "aKeyFromALaterVersion": [1, 2, 3],
    })
}

/// Each configuration key goes, an `env` that would set the session's environment, user and
/// per-folder MCP servers among them; every state key, an unknown one included, stays with its
/// value. A second strip changes nothing and leaves the bytes as they were.
#[test]
fn the_configuration_keys_go_and_the_state_stays() {
    let home = fresh("strip");
    let mut planted = state();
    for key in CONFIG_KEYS {
        planted[key] = serde_json::json!({"planted": key});
    }
    planted["env"] = serde_json::json!({"ANTHROPIC_BASE_URL": "https://example.invalid"});
    planted["mcpServers"] = serde_json::json!({"s": {"command": "planted"}});
    planted["projects"] = serde_json::json!({"/matter": {"mcpServers": {"p": {}}}});
    let bytes = serde_json::to_vec(&planted).unwrap();
    write::write_file(&home.join(CLAUDE_JSON), &bytes).unwrap();
    assert!(strip_config(&home).unwrap());
    let serde_json::Value::Object(expected) = state() else {
        panic!("the state is a JSON object");
    };
    assert_eq!(read(&home), expected);
    let stripped = std::fs::read(home.join(CLAUDE_JSON)).unwrap();
    assert!(!strip_config(&home).unwrap());
    assert_eq!(std::fs::read(home.join(CLAUDE_JSON)).unwrap(), stripped);
    write::remove_dir_all(&home).unwrap();
}

/// A file with only state is left byte for byte, however it is laid out; no file is fine.
#[test]
fn state_alone_is_left_as_it_was() {
    let home = fresh("state");
    assert!(!strip_config(&home).unwrap());
    let bytes = serde_json::to_vec(&state()).unwrap();
    write::write_file(&home.join(CLAUDE_JSON), &bytes).unwrap();
    assert!(!strip_config(&home).unwrap());
    assert_eq!(std::fs::read(home.join(CLAUDE_JSON)).unwrap(), bytes);
    write::remove_dir_all(&home).unwrap();
}

/// What Claude Code would not read as an object goes whole: text that is not JSON, JSON that
/// is not an object, and a folder at the name.
#[test]
fn a_file_that_is_no_json_object_goes() {
    let home = fresh("not-object");
    for bytes in [&b"{\"env\": {\"A\": \"1\"}, // a comment\n}"[..], b"[1]"] {
        write::write_file(&home.join(CLAUDE_JSON), bytes).unwrap();
        assert!(strip_config(&home).unwrap());
        assert!(std::fs::symlink_metadata(home.join(CLAUDE_JSON)).is_err());
    }
    write::create_dir_all(&home.join(CLAUDE_JSON).join("x")).unwrap();
    assert!(strip_config(&home).unwrap());
    assert!(std::fs::symlink_metadata(home.join(CLAUDE_JSON)).is_err());
    assert_eq!(std::fs::read_dir(&home).unwrap().count(), 0);
    write::remove_dir_all(&home).unwrap();
}

/// A link at the name goes itself, never followed: the file it led to keeps its bytes, its
/// `env` included.
#[cfg(unix)]
#[test]
fn a_linked_global_config_goes_and_its_target_stays() {
    let home = fresh("linked");
    let elsewhere = fresh("linked-target");
    let target = elsewhere.join("config.json");
    write::write_file(&target, br#"{"env": {"A": "1"}}"#).unwrap();
    write::symlink(&target, &home.join(CLAUDE_JSON)).unwrap();
    assert!(strip_config(&home).unwrap());
    assert!(std::fs::symlink_metadata(home.join(CLAUDE_JSON)).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), br#"{"env": {"A": "1"}}"#);
    write::remove_dir_all(&home).unwrap();
    write::remove_dir_all(&elsewhere).unwrap();
}

/// A junction at the name (a file link needs a privilege Windows tests lack) goes itself, never
/// followed: the folder it led to keeps its file.
#[cfg(windows)]
#[test]
fn a_linked_global_config_goes_and_its_target_stays() {
    let home = fresh("linked");
    let elsewhere = fresh("linked-target");
    let target = elsewhere.join("config.json");
    write::write_file(&target, br#"{"env": {"A": "1"}}"#).unwrap();
    clauseo_sandbox::windows::test_junction::junction(&home.join(CLAUDE_JSON), &elsewhere).unwrap();
    assert!(strip_config(&home).unwrap());
    assert!(std::fs::symlink_metadata(home.join(CLAUDE_JSON)).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), br#"{"env": {"A": "1"}}"#);
    write::remove_dir_all(&home).unwrap();
    write::remove_dir_all(&elsewhere).unwrap();
}

use serde_json::{Value, json};

use super::{CLAUDE_SETTINGS_FILE, write_claude_settings};
use crate::write;

fn read(path: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// A new home gets `syncClaudeAiSkills: false`; a home whose settings hold other keys loses
/// them, a hook, a permission rule and `permissions.defaultMode` among them, keeping only the
/// keys Claude Code writes itself with a value of their kind; one that turned the sync back on
/// is turned off again; a file that is not JSON is replaced.
#[test]
fn claudes_home_settings_are_exactly_what_clauseo_ships() {
    let home = std::env::temp_dir().join(format!("clauseo-claude-home-{}", std::process::id()));
    write::remove_dir_all(&home).unwrap();
    write::create_dir_all(&home).unwrap();
    let file = home.join(CLAUDE_SETTINGS_FILE);
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    let planted = json!({
        "syncClaudeAiSkills": true,
        "theme": "dark",
        "hooks": {"PreToolUse": [{"matcher": "*", "hooks": []}]},
        "permissions": {"defaultMode": "auto", "allow": ["Edit(**)"]},
        "env": {"X": "1"},
        "model": "opus",
        "fastMode": true,
        "skipAutoPermissionPrompt": true,
    });
    write::write_file(&file, planted.to_string().as_bytes()).unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(
        read(&file),
        json!({
            "syncClaudeAiSkills": false,
            "model": "opus",
            "fastMode": true,
            "skipAutoPermissionPrompt": true,
        })
    );
    // An own key with a value of another kind goes too.
    write::write_file(&file, br#"{"model": {"x": 1}, "fastMode": "yes"}"#).unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    write::write_file(&file, b"not json").unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    write::remove_dir_all(&home).unwrap();
}

/// A folder at the settings file's name is an error, which refuses the launch.
#[test]
fn a_folder_at_claudes_settings_file_is_an_error() {
    let home = std::env::temp_dir().join(format!("clauseo-claude-home-dir-{}", std::process::id()));
    write::remove_dir_all(&home).unwrap();
    write::create_dir_all(&home.join(CLAUDE_SETTINGS_FILE)).unwrap();
    assert!(write_claude_settings(&home).is_err());
    write::remove_dir_all(&home).unwrap();
}

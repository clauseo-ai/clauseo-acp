use serde_json::{Value, json};

use super::{CLAUDE_SETTINGS_FILE, write_claude_settings};
use crate::write;

fn read(path: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// A new home gets `syncClaudeAiSkills: false`; a home whose settings hold other keys keeps
/// them, and one that turned the sync back on is turned off again.
#[test]
fn claudes_home_settings_turn_off_synced_claude_ai_skills() {
    let home = std::env::temp_dir().join(format!("clauseo-claude-home-{}", std::process::id()));
    write::remove_dir_all(&home).unwrap();
    write::create_dir_all(&home).unwrap();
    let file = home.join(CLAUDE_SETTINGS_FILE);
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    write::write_file(&file, br#"{"syncClaudeAiSkills": true, "theme": "dark"}"#).unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(
        read(&file),
        json!({"syncClaudeAiSkills": false, "theme": "dark"})
    );
    write::write_file(&file, b"not json").unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    write::remove_dir_all(&home).unwrap();
}

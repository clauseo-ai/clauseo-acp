use serde_json::{Value, json};

use super::{CLAUDE_SETTINGS_FILE, claude_launch_env, write_claude_settings};
use crate::write;

/// A Claude launch's environment holds `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` whatever the
/// inherited environment, the agent's settings or the profile hook pass (`0`, `false`, in any
/// case of the name), so nothing turns Claude Code's auto-memory back on over the flag settings.
#[test]
fn a_claude_launch_keeps_auto_memory_off_whatever_is_inherited() {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    let var = crate::agent::loads::memory::AUTO_MEMORY_OFF_VAR;
    let agent = clauseo_settings::AgentLaunch {
        command: "claude-acp".to_owned(),
        args: Vec::new(),
        env: BTreeMap::from([(var.to_ascii_lowercase(), "false".to_owned())]),
    };
    let hook = BTreeMap::from([(OsString::from(var), OsString::from("0"))]);
    let home = std::env::temp_dir().join("clauseo-claude-home-env");
    let inherited = [(OsString::from(var), OsString::from("false"))];
    let env = crate::agent::launch_env(
        &agent,
        hook,
        claude_launch_env(&home),
        &[],
        inherited.into_iter(),
        &[],
    );
    let named: Vec<_> = env
        .iter()
        .filter(|(name, _)| {
            name.to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(var))
        })
        .collect();
    assert_eq!(named, [(&OsString::from(var), &OsString::from("1"))]);
}

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
    // An own key with a value of another kind goes too, and so does a model name that is too
    // long or holds what no model name does; one with a context suffix stays.
    write::write_file(&file, br#"{"model": {"x": 1}, "fastMode": "yes"}"#).unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    for model in [
        "a".repeat(129),
        "opus\n# x".to_owned(),
        "op us".to_owned(),
        String::new(),
    ] {
        write::write_file(&file, json!({"model": model}).to_string().as_bytes()).unwrap();
        write_claude_settings(&home).unwrap();
        assert_eq!(
            read(&file),
            json!({"syncClaudeAiSkills": false}),
            "{model:?}"
        );
    }
    write::write_file(&file, br#"{"model": "claude-opus-4-1[1m]"}"#).unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file)["model"], json!("claude-opus-4-1[1m]"));
    write::write_file(&file, b"not json").unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    write::remove_dir_all(&home).unwrap();
}

/// A folder at the settings file's name, with a file in it, is moved aside and the file written
/// fresh; nothing is left aside once it could be removed.
#[test]
fn a_folder_at_claudes_settings_file_is_moved_aside() {
    let home = std::env::temp_dir().join(format!("clauseo-claude-home-dir-{}", std::process::id()));
    write::remove_dir_all(&home).unwrap();
    write::create_dir_all(&home.join(CLAUDE_SETTINGS_FILE)).unwrap();
    write::write_file(&home.join(CLAUDE_SETTINGS_FILE).join("x"), b"x").unwrap();
    write_claude_settings(&home).unwrap();
    let file = home.join(CLAUDE_SETTINGS_FILE);
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    let names: Vec<_> = std::fs::read_dir(&home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, [CLAUDE_SETTINGS_FILE]);
    write::remove_dir_all(&home).unwrap();
}

/// When the folder at the name can be neither replaced nor renamed (the home not writable),
/// the write is an error, which refuses the launch. Skipped where the folder's mode does not
/// stop the writes (a root account).
#[cfg(unix)]
#[test]
fn a_settings_file_that_can_neither_be_replaced_nor_moved_is_an_error() {
    let home = std::env::temp_dir().join(format!("clauseo-claude-home-ro-{}", std::process::id()));
    write::remove_dir_all(&home).unwrap();
    write::create_dir_all(&home.join(CLAUDE_SETTINGS_FILE)).unwrap();
    write::set_mode(&home, 0o500).unwrap();
    let probe = home.join("probe");
    if write::create_dir_all(&probe).is_ok() {
        eprintln!("skipped: the folder's mode does not stop this account's writes");
    } else {
        assert!(write_claude_settings(&home).is_err());
        assert!(home.join(CLAUDE_SETTINGS_FILE).is_dir());
    }
    write::set_mode(&home, 0o700).unwrap();
    write::remove_dir_all(&home).unwrap();
}

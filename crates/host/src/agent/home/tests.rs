use serde_json::{Value, json};

use super::{
    CLAUDE_SETTINGS_FILE, claude_launch_env, write_claude_home_settings, write_claude_settings,
};
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

/// When the settings file is held open by another program that does not let it be deleted, it
/// can be neither replaced nor renamed: the write is an error, which refuses the launch, and
/// the file keeps its bytes.
#[cfg(windows)]
#[test]
fn a_settings_file_that_can_neither_be_replaced_nor_moved_is_an_error() {
    let home = std::env::temp_dir().join(format!("clauseo-claude-home-ro-{}", std::process::id()));
    write::remove_dir_all(&home).unwrap();
    write::create_dir_all(&home).unwrap();
    let file = home.join(CLAUDE_SETTINGS_FILE);
    write::write_file(&file, br#"{"hooks": {}}"#).unwrap();
    let open = write::open_deny_write(&file).unwrap();
    assert!(write_claude_settings(&home).is_err());
    drop(open);
    assert_eq!(std::fs::read(&file).unwrap(), br#"{"hooks": {}}"#);
    write::remove_dir_all(&home).unwrap();
}

/// A Clauseo home as `Locations::agent_home` lays it out, under a fresh data folder named for
/// `test`; the data folder and the home.
fn agent_home(test: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let data =
        std::env::temp_dir().join(format!("clauseo-claude-home-{test}-{}", std::process::id()));
    write::remove_dir_all(&data).unwrap();
    let home = data.join("agents").join("claude-acp").join("home");
    write::create_dir_all(&home).unwrap();
    (data, home)
}

/// Before a sign-in, status check or logout, the app's write also clears what a launch clears
/// of Claude's settings: `.claude.json`'s `env` goes and its state stays, and `.config.json`
/// goes, before the settings file is written.
#[test]
fn the_sign_in_write_clears_the_global_config_and_the_settings_files() {
    let (data, home) = agent_home("sign-in");
    let config = br#"{"env": {"ANTHROPIC_BASE_URL": "https://example.invalid"}, "userID": "u"}"#;
    write::write_file(&home.join(".claude.json"), config).unwrap();
    write::write_file(&home.join(".config.json"), br#"{"env": {"A": "1"}}"#).unwrap();
    write_claude_home_settings(&home).unwrap();
    assert_eq!(read(&home.join(".claude.json")), json!({"userID": "u"}));
    assert!(std::fs::symlink_metadata(home.join(".config.json")).is_err());
    assert_eq!(
        read(&home.join(CLAUDE_SETTINGS_FILE)),
        json!({"syncClaudeAiSkills": false})
    );
    write::remove_dir_all(&data).unwrap();
}

/// A home swapped for a link (a symlink on Unix, a junction on Windows) makes the app's write
/// an error, so the sign-in job starts no Claude Code; the folder it leads to keeps every file.
#[cfg(any(unix, windows))]
#[test]
fn a_sign_in_write_into_a_linked_home_is_an_error_and_its_target_stays() {
    let (data, home) = agent_home("sign-in-linked");
    let elsewhere = data.join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    write::write_file(&elsewhere.join(".config.json"), b"kept").unwrap();
    write::write_file(&elsewhere.join(".claude.json"), br#"{"env": {}}"#).unwrap();
    write::remove_dir_all(&home).unwrap();
    #[cfg(unix)]
    write::symlink(&elsewhere, &home).unwrap();
    #[cfg(windows)]
    clauseo_sandbox::windows::test_junction::junction(&home, &elsewhere).unwrap();
    assert!(write_claude_home_settings(&home).is_err());
    assert_eq!(
        std::fs::read(elsewhere.join(".config.json")).unwrap(),
        b"kept"
    );
    assert_eq!(
        std::fs::read(elsewhere.join(".claude.json")).unwrap(),
        br#"{"env": {}}"#
    );
    assert!(std::fs::symlink_metadata(elsewhere.join(CLAUDE_SETTINGS_FILE)).is_err());
    write::remove_link(&home).unwrap();
    write::remove_dir_all(&data).unwrap();
}

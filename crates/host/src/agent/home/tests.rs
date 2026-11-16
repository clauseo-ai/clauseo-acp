use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{
    CLAUDE_COWORK_SETTINGS_FILE, CLAUDE_SETTINGS_FILE, claude_settings_intact,
    write_claude_settings,
};
use crate::write;

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// A fresh, empty home for `test`.
fn home(test: &str) -> PathBuf {
    let home =
        std::env::temp_dir().join(format!("clauseo-claude-home-{test}-{}", std::process::id()));
    write::remove_dir_all(&home).unwrap();
    write::create_dir_all(&home).unwrap();
    home
}

/// Each launch writes `syncClaudeAiSkills: false` and nothing else: other keys, a sync turned
/// back on and a file that is not JSON are all replaced, never merged.
#[test]
fn claudes_home_settings_hold_only_clauseos_keys() {
    let home = home("only");
    let file = home.join(CLAUDE_SETTINGS_FILE);
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    let planted = json!({
        "syncClaudeAiSkills": true,
        "theme": "dark",
        "disableAllHooks": true,
        "permissions": {"allow": ["Bash"], "defaultMode": "bypassPermissions"},
        "env": {"IS_SANDBOX": "1"},
    });
    write::write_file(&file, planted.to_string().as_bytes()).unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    write::write_file(&file, b"not json").unwrap();
    write_claude_settings(&home).unwrap();
    assert_eq!(read(&file), json!({"syncClaudeAiSkills": false}));
    write::remove_dir_all(&home).unwrap();
}

/// A planted `cowork_settings.json` is removed, and a link planted at `settings.json` is
/// replaced by a file: what it pointed at is left as it was.
#[cfg(unix)]
#[test]
fn a_planted_link_and_cowork_file_are_replaced_not_followed() {
    let home = home("link");
    let elsewhere = home.join("elsewhere.json");
    write::write_file(&elsewhere, b"{\"keep\": true}").unwrap();
    write::symlink(&elsewhere, &home.join(CLAUDE_SETTINGS_FILE)).unwrap();
    write::write_file(&home.join(CLAUDE_COWORK_SETTINGS_FILE), b"{}").unwrap();
    write_claude_settings(&home).unwrap();
    let meta = std::fs::symlink_metadata(home.join(CLAUDE_SETTINGS_FILE)).unwrap();
    assert!(meta.is_file());
    assert_eq!(read(&elsewhere), json!({"keep": true}));
    assert!(!home.join(CLAUDE_COWORK_SETTINGS_FILE).exists());
    assert!(claude_settings_intact(&home));
    write::remove_dir_all(&home).unwrap();
}

/// The compare helper: what the launch wrote matches, the same object spaced otherwise still
/// matches (Claude Code's own writer may reformat it), and any change does not: a key added or
/// changed, a file that is no JSON or gone, a cowork file beside it, a link in its place.
#[test]
fn the_compare_helper_says_whether_the_file_is_still_what_the_launch_wrote() {
    let home = home("compare");
    let file = home.join(CLAUDE_SETTINGS_FILE);
    assert!(!claude_settings_intact(&home), "no file yet");
    write_claude_settings(&home).unwrap();
    assert!(claude_settings_intact(&home));
    write::write_file(&file, b"{\"syncClaudeAiSkills\":false}\n").unwrap();
    assert!(
        claude_settings_intact(&home),
        "the same object, spaced otherwise"
    );
    for changed in [
        r#"{"syncClaudeAiSkills": false, "disableAllHooks": true}"#,
        r#"{"syncClaudeAiSkills": true}"#,
        "{}",
        "not json",
    ] {
        write::write_file(&file, changed.as_bytes()).unwrap();
        assert!(!claude_settings_intact(&home), "{changed}");
    }
    write_claude_settings(&home).unwrap();
    write::write_file(&home.join(CLAUDE_COWORK_SETTINGS_FILE), b"{}").unwrap();
    assert!(!claude_settings_intact(&home), "a cowork file");
    write_claude_settings(&home).unwrap();
    write::remove_file(&file).unwrap();
    assert!(!claude_settings_intact(&home), "gone");
    #[cfg(unix)]
    {
        write_claude_settings(&home).unwrap();
        let copy = home.join("copy.json");
        write::write_new_private_file(&copy, &std::fs::read(&file).unwrap()).unwrap();
        write::remove_file(&file).unwrap();
        write::symlink(&copy, &file).unwrap();
        assert!(!claude_settings_intact(&home), "a link to the same text");
    }
    write::remove_dir_all(&home).unwrap();
}

/// The keys Claude Code writes to its user file itself pass, alone and together, with safe
/// values; every other key, a planted one or one of theirs with a loosening value, fails.
#[test]
fn claude_codes_own_writes_pass_the_compare_and_nothing_else_does() {
    let home = home("own");
    let file = home.join(CLAUDE_SETTINGS_FILE);
    let check = |extra: &Value| {
        let mut settings = json!({"syncClaudeAiSkills": false});
        for (key, value) in extra.as_object().unwrap() {
            settings[key] = value.clone();
        }
        write::write_file(&file, settings.to_string().as_bytes()).unwrap();
        claude_settings_intact(&home)
    };
    let tolerated = [
        json!({"model": "opus[1m]"}),
        json!({"model": "sonnet"}),
        json!({"fastMode": true}),
        json!({"fastMode": false}),
        json!({"skipAutoPermissionPrompt": true}),
        json!({"permissions": {"defaultMode": "auto"}}),
        json!({"permissions": {"defaultMode": "default"}}),
        json!({"permissions": {"defaultMode": "acceptEdits"}}),
        json!({"permissions": {"defaultMode": "plan"}}),
        json!({
            "model": "opus",
            "fastMode": true,
            "skipAutoPermissionPrompt": true,
            "permissions": {"defaultMode": "auto"},
        }),
    ];
    for extra in &tolerated {
        assert!(check(extra), "{extra}");
    }
    let refused = [
        json!({"permissions": {"defaultMode": "bypassPermissions"}}),
        json!({"permissions": {"defaultMode": "dontAsk"}}),
        json!({"permissions": {"defaultMode": "auto", "allow": ["Bash"]}}),
        json!({"permissions": {"allow": ["Bash(curl:*)"]}}),
        json!({"permissions": {"deny": []}}),
        // unix-paths-ok: a planted settings or env value; it never reaches a path API
        json!({"permissions": {"additionalDirectories": ["/Users/lawyer"]}}),
        json!({"permissions": {}}),
        json!({"sandbox": {"enabled": false}}),
        json!({"hooks": {"PreToolUse": []}}),
        json!({"disableAllHooks": true}),
        json!({"env": {"IS_SANDBOX": "1"}}),
        json!({"apiKeyHelper": "/tmp/key.sh"}),
        json!({"skipDangerousModePermissionPrompt": true}),
        json!({"model": 1}),
        json!({"fastMode": "yes"}),
        json!({"skipAutoPermissionPrompt": null}),
        json!({"syncClaudeAiSkills": true, "model": "opus"}),
    ];
    for extra in &refused {
        assert!(!check(extra), "{extra}");
    }
    write::write_file(&file, br#"{"model": "opus"}"#).unwrap();
    assert!(!claude_settings_intact(&home), "Clauseo's own key gone");
    write::remove_dir_all(&home).unwrap();
}

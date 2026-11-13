//! The Windows shell deny rules for the data folder, on every OS (pure text): the spellings of
//! `<data>` and of a configured folder, escapes, and Claude Code's matching emulated. The
//! launch-sized cases are in `shell_deny_launch_tests.rs`.

use std::ffi::OsString;
use std::path::PathBuf;

use super::windows_shell_deny_rules;
use crate::agent::data_fence::LaunchDeny;

const DATA: &str = r"C:\Users\Asha\AppData\Local\ai.clauseo.desktop";

pub(super) fn env(name: &str) -> Option<OsString> {
    match name {
        "LOCALAPPDATA" => Some(r"c:\users\asha\appdata\local\".into()),
        "APPDATA" => Some(r"C:\Users\Asha\AppData\Roaming".into()),
        _ => None,
    }
}

/// Whether `rule` matches `command` as Claude Code 2.1.293's `hQ` does: `\*` a literal `*`,
/// `\\` a literal `\`, any other `*` any text; PowerShell rules in any case.
pub(super) fn matches(rule: &str, command: &str) -> bool {
    let (tool, content) = rule.split_once('(').unwrap();
    let content = content.strip_suffix(')').unwrap();
    let insensitive = tool == "PowerShell";
    let mut parts: Vec<String> = vec![String::new()];
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('*' | '\\')) => {
                parts.last_mut().unwrap().push(chars.next().unwrap());
            }
            '*' => parts.push(String::new()),
            _ => parts.last_mut().unwrap().push(c),
        }
    }
    let fold = |s: &str| {
        if insensitive {
            s.to_lowercase()
        } else {
            s.to_owned()
        }
    };
    let text = fold(command);
    let parts: Vec<String> = parts.iter().map(|p| fold(p)).collect();
    let (first, rest) = parts.split_first().unwrap();
    let Some(mut tail) = text.strip_prefix(first.as_str()) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return tail.is_empty();
    };
    for part in middle {
        let Some((_, after)) = tail.split_once(part.as_str()) else {
            return false;
        };
        tail = after;
    }
    tail.ends_with(last.as_str())
}

/// Whether any of `rules` for `tool` matches `command`.
pub(super) fn denied_by(rules: &[String], tool: &str, command: &str) -> bool {
    rules
        .iter()
        .filter(|rule| rule.starts_with(&format!("{tool}(")))
        .any(|rule| matches(rule, command))
}

fn denied(tool: &str, command: &str) -> bool {
    let rules = windows_shell_deny_rules(&LaunchDeny::fixed(vec![PathBuf::from(DATA)]), &env);
    denied_by(&rules, tool, command)
}

/// The fixed targets in every spelling: the path with `\`, `/` and `\\`, Git Bash's `/c/…`, the
/// variables, `~` and Clauseo's folder name alone, in both shells.
#[test]
fn every_spelling_of_the_data_folder_is_denied_in_both_shells() {
    for command in [
        r"type C:\Users\Asha\AppData\Local\ai.clauseo.desktop\agents\signin\auth.json",
        "cat C:/Users/Asha/AppData/Local/ai.clauseo.desktop/settings.json",
        r"cat C:\\Users\\Asha\\AppData\\Local\\ai.clauseo.desktop\\settings.json",
        "cat /c/Users/Asha/AppData/Local/ai.clauseo.desktop/state.sqlite",
        r"type %LOCALAPPDATA%\ai.clauseo.desktop\settings.json",
        "type %LOCALAPPDATA%/ai.clauseo.desktop/settings.json",
        r"Get-Content $env:LOCALAPPDATA\ai.clauseo.desktop\settings.json",
        "Get-Content $env:LOCALAPPDATA/ai.clauseo.desktop/settings.json",
        "cat $LOCALAPPDATA/ai.clauseo.desktop/settings.json",
        "cat ${LOCALAPPDATA}/ai.clauseo.desktop/settings.json",
        "cat ~/AppData/Local/ai.clauseo.desktop/settings.json",
        "python3 -c \"open('ai.clauseo.desktop/settings.json')\"",
        "cd /tmp && cat C:/Users/Asha/AppData/Local/ai.clauseo.desktop/logs/a.log",
        r"type C:\Users\Asha\AppData\Local\ai.clauseo.desktop\launch-settings\x.json",
        // The folder itself, named alone, and a glob or `..` in it.
        r#"dir "C:\Users\Asha\AppData\Local\ai.clauseo.desktop""#,
        "ls C:/Users/Asha/AppData/Local/ai.clauseo.desktop/",
        r"Get-ChildItem $env:LOCALAPPDATA\ai.clauseo.desktop -Recurse",
        "ls ~/AppData/Local/ai.clauseo.desktop",
        "cat ai.clauseo.desktop/*/x",
        r"type ai.clauseo.desktop\run\own\..\..\settings.json",
    ] {
        assert!(denied("Bash", command), "Bash: {command}");
        assert!(denied("PowerShell", command), "PowerShell: {command}");
    }
    // PowerShell rules match in any case; Bash rules only as written (the residual).
    let lower = r"get-content $ENV:localappdata\AI.CLAUSEO.DESKTOP\settings.json";
    assert!(denied("PowerShell", lower));
    assert!(!denied(
        "Bash",
        "cat /C/USERS/ASHA/APPDATA/LOCAL/AI.CLAUSEO.DESKTOP/settings.json"
    ));
    // Other folders are untouched, and so is what the launch did not name.
    for command in [
        "dir C:\\Users\\Asha\\Documents\\Clauseo",
        "ls ~/AppData/Local/Temp",
        r"type C:\Users\Asha\AppData\Local\ai.clauseo.desktop\tools\x.txt",
        "ls C:/Users/Asha/AppData/Local/ai.clauseo.desktop/profile/claude-plugin/skills",
    ] {
        assert!(!denied("Bash", command), "{command}");
        assert!(!denied("PowerShell", command), "{command}");
    }
}

/// A configured folder: its paths and the variable it sits under, never Clauseo's own name nor
/// `%LOCALAPPDATA%`.
#[test]
fn a_configured_folder_gets_its_paths_and_the_variable_it_sits_under() {
    let data = PathBuf::from(r"C:\Users\Asha\AppData\Roaming\Firm Data\clauseo");
    let rules = windows_shell_deny_rules(&LaunchDeny::fixed(vec![data]), &env);
    for command in [
        r"type C:\Users\Asha\AppData\Roaming\Firm Data\clauseo\settings.json",
        r"cat C:\\Users\\Asha\\AppData\\Roaming\\Firm\ Data\\clauseo\\settings.json",
        "cat 'C:/Users/Asha/AppData/Roaming/Firm Data/clauseo/logs/x'",
        "cat '/c/Users/Asha/AppData/Roaming/Firm Data/clauseo/state.sqlite-wal'",
        r#"type "%APPDATA%\Firm Data\clauseo\settings.json""#,
        r#"Get-Content "$env:APPDATA/Firm Data/clauseo/settings.json""#,
        r#"cat "${APPDATA}/Firm Data/clauseo/settings.json""#,
        r#"ls "$APPDATA/Firm Data/clauseo""#,
    ] {
        let both = ["Bash", "PowerShell"];
        // The escaped form's space is `\ `, a form no rule spells: that one is the residual.
        if command.contains(r"Firm\ Data") {
            assert!(!denied_by(&rules, "Bash", command), "{command}");
            continue;
        }
        for tool in both {
            assert!(denied_by(&rules, tool, command), "{tool}: {command}");
        }
    }
    assert!(!rules.iter().any(|r| r.contains("ai.clauseo.desktop")));
    assert!(!rules.iter().any(|r| r.contains("LOCALAPPDATA")));
    assert_eq!(rules.len() % 2, 0, "each rule once per shell: {rules:#?}");
}

#[test]
fn a_share_path_and_a_star_are_escaped() {
    let fixed = |path: &str| LaunchDeny::fixed(vec![PathBuf::from(path)]);
    let rules = windows_shell_deny_rules(&fixed(r"\\?\C:\d"), &|_| None);
    assert!(
        rules.contains(&r"Bash(*C:\\d\\settings.json*)".to_owned()),
        "{rules:#?}"
    );
    let rules = windows_shell_deny_rules(&fixed(r"\\srv\share\d"), &|_| None);
    assert!(
        rules.contains(&r"Bash(*\\\\srv\\share\\d\\settings.json*)".to_owned()),
        "{rules:#?}"
    );
    assert!(denied_by(
        &rules,
        "Bash",
        r"type \\srv\share\d\settings.json"
    ));
    // A target's pattern `*` stays Claude's `*`; a name's escaped one is a literal `\*`.
    let mut deny = fixed(r"C:\d");
    deny.targets = vec![r"made\*later.txt".to_owned()];
    let rules = windows_shell_deny_rules(&deny, &|_| None);
    assert!(
        rules.contains(&r"Bash(*C:\\d\\made\*later.txt*)".to_owned()),
        "{rules:#?}"
    );
    let deny = fixed(r"C:\d");
    let rules = windows_shell_deny_rules(&deny, &|_| None);
    assert!(
        rules.contains(&r"Bash(*C:\\d\\state.sqlite*)".to_owned()),
        "{rules:#?}"
    );
    assert!(
        rules.contains(&r"Bash(*C:\\d\\run\\*\\relay-token*)".to_owned()),
        "{rules:#?}"
    );
    // No rule holds a globstar (`/**/`), which Claude Code reads otherwise.
    assert!(
        !rules.iter().any(|rule| rule.contains("/**/")),
        "{rules:#?}"
    );
}

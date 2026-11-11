//! The Windows shell deny rules for the data folder, on every OS (pure text).

use std::ffi::OsString;
use std::path::PathBuf;

use super::windows_shell_deny_rules;

const DATA: &str = r"C:\Users\Asha\AppData\Local\ai.clauseo.desktop";

fn env(name: &str) -> Option<OsString> {
    match name {
        "LOCALAPPDATA" => Some(r"c:\users\asha\appdata\local\".into()),
        "APPDATA" => Some(r"C:\Users\Asha\AppData\Roaming".into()),
        _ => None,
    }
}

/// Whether `rule` matches `command` as Claude Code 2.1.293's `hQ` does: `\*` a literal `*`,
/// `\\` a literal `\`, any other `*` any text; PowerShell rules in any case.
fn matches(rule: &str, command: &str) -> bool {
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

fn denied(tool: &str, command: &str) -> bool {
    let rules = windows_shell_deny_rules(&[PathBuf::from(DATA)], &env);
    rules
        .iter()
        .filter(|rule| rule.starts_with(&format!("{tool}(")))
        .any(|rule| matches(rule, command))
}

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
        "python3 -c \"open('ai.clauseo.desktop/x')\"",
        "cd /tmp && cat C:/Users/Asha/AppData/Local/ai.clauseo.desktop/logs/a.log",
    ] {
        assert!(denied("Bash", command), "Bash: {command}");
        assert!(denied("PowerShell", command), "PowerShell: {command}");
    }
    // PowerShell rules match in any case; Bash rules only as written (the residual).
    let lower = r"get-content $ENV:localappdata\AI.CLAUSEO.DESKTOP\settings.json";
    assert!(denied("PowerShell", lower));
    assert!(!denied(
        "Bash",
        "cat /C/USERS/ASHA/APPDATA/LOCAL/AI.CLAUSEO.DESKTOP/x"
    ));
    // Other folders are untouched.
    for command in [
        "dir C:\\Users\\Asha\\Documents\\Clauseo",
        "ls ~/AppData/Local/Temp",
    ] {
        assert!(!denied("Bash", command), "{command}");
        assert!(!denied("PowerShell", command), "{command}");
    }
}

#[test]
fn a_configured_folder_gets_its_paths_and_the_variable_it_sits_under() {
    let data = PathBuf::from(r"C:\Users\Asha\AppData\Roaming\Firm Data\clauseo");
    let rules = windows_shell_deny_rules(&[data], &env);
    for want in [
        r"Bash(*C:\\Users\\Asha\\AppData\\Roaming\\Firm Data\\clauseo*)",
        "Bash(*C:/Users/Asha/AppData/Roaming/Firm Data/clauseo*)",
        "Bash(*/c/Users/Asha/AppData/Roaming/Firm Data/clauseo*)",
        r"PowerShell(*%APPDATA%\\Firm Data\\clauseo*)",
        "PowerShell(*$env:APPDATA/Firm Data/clauseo*)",
        "Bash(*${APPDATA}/Firm Data/clauseo*)",
    ] {
        assert!(rules.iter().any(|r| r == want), "{want} in {rules:#?}");
    }
    // Not Clauseo's own name and not under %LOCALAPPDATA%: neither form is there.
    assert!(!rules.iter().any(|r| r.contains("ai.clauseo.desktop")));
    assert!(!rules.iter().any(|r| r.contains("LOCALAPPDATA")));
    assert_eq!(rules.len() % 2, 0, "each form once per shell: {rules:#?}");
}

#[test]
fn a_share_path_and_a_star_are_escaped() {
    let rules = windows_shell_deny_rules(&[PathBuf::from(r"\\?\C:\d")], &|_| None);
    assert!(rules.contains(&r"Bash(*C:\\d*)".to_owned()), "{rules:#?}");
    let rules = windows_shell_deny_rules(&[PathBuf::from(r"\\srv\share\d")], &|_| None);
    assert!(
        rules.contains(&r"Bash(*\\\\srv\\share\\d*)".to_owned()),
        "{rules:#?}"
    );
    assert!(matches(&rules[0], r"type \\srv\share\d\settings.json"));
}

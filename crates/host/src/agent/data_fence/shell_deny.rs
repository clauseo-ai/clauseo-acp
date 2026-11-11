//! Windows: Claude's `Bash(…)` and `PowerShell(…)` deny rules for a command that names the data
//! folder (decision record 0030, "Claude's permission mode"). Claude runs in `auto` on Windows
//! too, with no sandbox there, so a command in no rule goes to the classifier alone; these rules
//! refuse the plain spellings of `<data>` before it sees them, beside the `Read` and `Edit`
//! rules of [`super::claude_deny_rules`].
//!
//! Claude Code 2.1.293 matches a Bash or PowerShell rule's `*` anywhere, spaces included, a
//! leading one too (code.claude.com/docs/en/permissions, "Wildcard patterns"); in a rule `\*` is
//! a literal `*` and `\\` a literal `\`, any other `\` stands for itself (the binary's `hQ`).
//! Bash rules match case-sensitively, PowerShell rules in any case (`preparePermissionMatcher`
//! passes `hQ` its case-insensitive flag). A scoped PowerShell rule also keeps the PowerShell
//! tool on beside the Bash deny rules, which would otherwise turn it off (code.claude.com/docs/
//! en/tools-reference, "Bash deny rules also turn off the PowerShell tool").
//!
//! A deny list is not a fence: a path built at run time, a variable of the agent's own, a `cd`
//! first, other letter cases in Bash or a script that opens the file are not caught. That is
//! the accepted residual of `auto` on Windows (decision record 0030).

use std::ffi::OsString;
use std::path::PathBuf;

/// The variables a command may spell `<data>`'s parent with: `%LOCALAPPDATA%` holds the default
/// data folder, `%APPDATA%` a configured one may sit under.
const VARS: [&str; 2] = ["LOCALAPPDATA", "APPDATA"];

/// `Bash(*<form>*)` and `PowerShell(*<form>*)` for each way a command may write each of
/// `spellings` of `<data>`: its path with `\`, with `/`, with `\\` (Git Bash's escaped form)
/// and as Git Bash's `/c/…`; under a folder that one of [`VARS`] names (read with `var`, in any
/// case), `%VAR%`, `$env:VAR`, `$VAR` and `${VAR}` followed by the rest with `\` or `/`; and,
/// when the folder has Clauseo's own name (`clauseo_settings::IDENTIFIER`), that name alone,
/// which no other folder has. A spelling that is not UTF-8 gets none.
pub(crate) fn windows_shell_deny_rules(
    spellings: &[PathBuf],
    var: &dyn Fn(&str) -> Option<OsString>,
) -> Vec<String> {
    let mut forms: Vec<String> = Vec::new();
    let mut push = |form: String| {
        if !form.is_empty() && !forms.contains(&form) {
            forms.push(form);
        }
    };
    for spelling in spellings {
        let Some(text) = spelling.to_str() else {
            continue;
        };
        let back = drive_form(text);
        let back = back.trim_end_matches('\\');
        let forward = back.replace('\\', "/");
        push(back.to_owned());
        push(forward.clone());
        push(back.replace('\\', "\\\\"));
        let mut chars = forward.chars();
        if let (Some(drive), Some(':')) = (chars.next(), chars.next())
            && drive.is_ascii_alphabetic()
        {
            push(format!("/{}{}", drive.to_ascii_lowercase(), chars.as_str()));
        }
        for name in VARS {
            let Some(value) = var(name).and_then(|v| v.into_string().ok()) else {
                continue;
            };
            let value = drive_form(&value);
            let value = value.trim_end_matches('\\');
            let Some(rest) = strip_prefix_folded(back, value).and_then(|r| r.strip_prefix('\\'))
            else {
                continue;
            };
            if value.is_empty() || rest.is_empty() {
                continue;
            }
            let rest_forward = rest.replace('\\', "/");
            for lead in [format!("%{name}%"), format!("$env:{name}")] {
                push(format!("{lead}\\{rest}"));
                push(format!("{lead}/{rest_forward}"));
            }
            for lead in [format!("${name}"), format!("${{{name}}}")] {
                push(format!("{lead}/{rest_forward}"));
            }
        }
        // The last name, split by hand: a Windows path is one name to `Path` off Windows.
        if back.rsplit('\\').next() == Some(clauseo_settings::IDENTIFIER) {
            push(clauseo_settings::IDENTIFIER.to_owned());
        }
    }
    let mut rules = Vec::new();
    for tool in ["Bash", "PowerShell"] {
        for form in &forms {
            rules.push(format!("{tool}(*{}*)", escape(form)));
        }
    }
    rules
}

/// `text` in its drive form (`\\?\C:\x` is `C:\x`), `/` read as `\`.
fn drive_form(text: &str) -> String {
    crate::network_paths::resolve::drive_form(text).unwrap_or_else(|| text.replace('/', "\\"))
}

/// `text` without `prefix`, compared in any ASCII case, as Windows compares names.
fn strip_prefix_folded<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    let rest = text.get(prefix.len()..)?;
    head.eq_ignore_ascii_case(prefix).then_some(rest)
}

/// `text` in a Bash or PowerShell rule: `\` and `*` escaped by `\`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
#[path = "shell_deny_tests.rs"]
mod tests;

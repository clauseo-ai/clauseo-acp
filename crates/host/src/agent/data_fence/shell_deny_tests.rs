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

/// The first (or with `last` the last) `c` in `text` that is not escaped, as Claude Code 2.1.293's
/// `p` and `m` beside `lYe` find it: one with an even run of backslashes before it.
fn unescaped(text: &str, c: char, last: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let free = |at: usize| {
        let run = bytes
            .iter()
            .take(at)
            .rev()
            .take_while(|b| **b == b'\\')
            .count();
        run % 2 == 0
    };
    let mut hits = text
        .match_indices(c)
        .map(|(at, _)| at)
        .filter(|at| free(*at));
    if last { hits.next_back() } else { hits.next() }
}

/// `rule` as Claude Code 2.1.293's settings loader reads it: its tool and its content after `Lho`,
/// or why the loader skips it ("Invalid permission rule … was skipped", `h_`), ported from
/// `dCe` for the tools these rules name:
/// - empty or blank: skipped;
/// - `lYe`: the first unescaped `(` ([`unescaped`]) ends the tool's name, which holds no `(` or
///   `)`, and the last unescaped `)` must end the rule, else "Malformed Tool(content) rule" (a
///   rule whose content ends in an odd run of backslashes escapes its own closing `)`);
/// - empty parentheses, or a tool name that does not start upper case: skipped;
/// - `om`: a `\(` or `\)` after a character other than `\` (`nm`) while the unescaped `(` and `)`
///   differ in number (`ea`): skipped as ambiguous;
/// - `jn`: a content of `*` is the whole tool, which this module never means (here an error);
/// - `Bash` with a content of `:*`, or a file tool (`Read`, `Edit`, `Write`, …) whose content
///   holds `:*`: skipped.
pub(super) fn parse(rule: &str) -> Result<(String, String), &'static str> {
    if rule.trim().is_empty() {
        return Err("empty");
    }
    let open = unescaped(rule, '(', false);
    let close = unescaped(rule, ')', true);
    let (Some(open), Some(close)) = (open, close) else {
        return Err(if open.is_none() && close.is_none() {
            "a whole-tool rule"
        } else {
            "malformed"
        });
    };
    let tool = rule.get(..open).unwrap();
    if close <= open || close != rule.len() - 1 || tool.contains(['(', ')']) {
        return Err("malformed");
    }
    let raw = rule.get(open + 1..close).unwrap();
    if raw.is_empty() {
        return Err("empty parentheses");
    }
    if !tool.contains('_') && !tool.starts_with(|c: char| c.is_ascii_uppercase()) {
        return Err("a tool name not upper case");
    }
    let bytes = raw.as_bytes();
    let lone_escape = (0..bytes.len().saturating_sub(1)).any(|at| {
        let before = at.checked_sub(1).and_then(|b| bytes.get(b));
        bytes.get(at) == Some(&b'\\')
            && matches!(bytes.get(at + 1), Some(b'(' | b')'))
            && before != Some(&b'\\')
    });
    let count = |c: char| {
        raw.match_indices(c)
            .filter(|(at, _)| {
                bytes
                    .iter()
                    .take(*at)
                    .rev()
                    .take_while(|b| **b == b'\\')
                    .count()
                    % 2
                    == 0
            })
            .count()
    };
    if lone_escape && count('(') != count(')') {
        return Err("ambiguous");
    }
    if raw == "*" {
        return Err("a whole-tool rule");
    }
    // `Lho`.
    let content = raw
        .replace("\\(", "(")
        .replace("\\)", ")")
        .replace("\\\\", "\\");
    let file_tool = [
        "Read",
        "Write",
        "Edit",
        "Glob",
        "NotebookRead",
        "NotebookEdit",
        "Cd",
    ];
    if (tool == "Bash" && content == ":*") || (file_tool.contains(&tool) && content.contains(":*"))
    {
        return Err("a :* the tool refuses");
    }
    Ok((tool.to_owned(), content))
}

/// A Bash or PowerShell rule as Claude Code 2.1.293 reads it, ported from the pinned binary
/// (`@anthropic-ai/claude-agent-sdk-linux-x64` 0.3.293, `package/claude`; `strings` shows the
/// bundle's JS):
/// - [`parse`]: the settings loader's checks, and `lYe`'s split at the first unescaped `(` and
///   the last unescaped `)`, which must end the rule; a rule it skips is no rule;
/// - `cCe`: a content `^(.+):\*$` is a legacy prefix rule, matching a command that is the rest
///   or starts with it and a space (also after `xargs `), its `*`s literal;
/// - `jn`: an empty content or `*` is the whole tool; otherwise
///   `Lho(e) = Nho(e).replaceAll("\\\\", "\\")` with
///   `Nho(e) = e.replaceAll("\\(", "(").replaceAll("\\)", ")")`: `\(`, `\)` are parentheses and
///   every `\\` becomes `\`, before any matching;
/// - `hQ(pattern, command, caseInsensitive, true)` (Bash through `GM`, case-sensitive; PowerShell
///   through `yt`, in any case): the pattern trimmed, runs of spaces and tabs in both made one
///   space; `\*` a literal `*`, `\\` a literal `\`, any other `\` itself, every other `*` any text
///   (`.*`, newlines included); a `/**/` a globstar; a pattern ending in ` *` with no other `*`
///   also matches without the ` …`; the whole command matched (`^…$`).
///
/// The pinned binary itself, driven through a local stand-in for the Messages API
/// (`scripts/claude-rule-probe`), agreed with this port on every case it was given.
pub(super) struct Rule {
    tool: String,
    insensitive: bool,
    /// The literal text between the pattern's `*`s (folded when `insensitive`).
    parts: Vec<String>,
    /// The pattern ends in ` *` and has no other `*`.
    optional_tail: bool,
    /// A legacy prefix rule (`cCe`: a content `^(.+):\*$`): the command is this text or starts
    /// with it and a space, also after `xargs ` (folded when `insensitive`).
    prefix: Option<String>,
}

impl Rule {
    /// The rule, or `None` when Claude Code's loader would skip it ([`parse`]).
    pub(super) fn new(rule: &str) -> Option<Self> {
        let (tool, content) = parse(rule).ok()?;
        assert!(!content.contains('\0'));
        let insensitive = tool == "PowerShell";
        let fold = |text: &str| {
            if insensitive {
                text.to_lowercase()
            } else {
                text.to_owned()
            }
        };
        if let Some(prefix) = content.strip_suffix(":*").filter(|rest| !rest.is_empty()) {
            return Some(Self {
                tool,
                insensitive,
                parts: Vec::new(),
                optional_tail: false,
                prefix: Some(fold(&squeeze(prefix))),
            });
        }
        let pattern = squeeze(content.trim());
        let mut parts = vec![String::new()];
        let mut wildcards = 0;
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if matches!(chars.peek(), Some('*' | '\\')) => {
                    parts.last_mut().unwrap().push(chars.next().unwrap());
                }
                '*' => {
                    wildcards += 1;
                    parts.push(String::new());
                }
                _ => parts.last_mut().unwrap().push(c),
            }
        }
        // No rule here may hold a globstar, which `hQ` reads otherwise.
        assert!(
            !parts
                .windows(3)
                .any(|w| w[0].ends_with('/') && w[1].is_empty() && w[2].starts_with('/')),
            "a globstar: {rule}"
        );
        let optional_tail = wildcards == 1
            && parts.last().is_some_and(String::is_empty)
            && parts.first().is_some_and(|first| first.ends_with(' '));
        let parts = parts
            .iter()
            .map(|part| {
                if insensitive {
                    part.to_lowercase()
                } else {
                    part.clone()
                }
            })
            .collect();
        Some(Self {
            tool,
            insensitive,
            parts,
            optional_tail,
            prefix: None,
        })
    }

    /// The tool the rule is for.
    pub(super) fn tool(&self) -> &str {
        &self.tool
    }

    /// The longest literal text a command must hold to match, or `None` for a prefix rule.
    pub(super) fn key(&self) -> Option<&str> {
        if self.prefix.is_some() {
            return None;
        }
        self.parts
            .iter()
            .max_by_key(|part| part.len())
            .map(String::as_str)
    }

    /// The literal texts between the pattern's `*`s (none for a prefix rule).
    pub(super) fn parts(&self) -> &[String] {
        &self.parts
    }

    /// Whether this rule matches `command` for `tool`.
    pub(super) fn hits(&self, tool: &str, command: &str) -> bool {
        self.tool == tool && self.hits_prepared(&prepare(command, self.insensitive))
    }

    /// Whether this rule matches a command already passed through [`prepare`] for it.
    pub(super) fn hits_prepared(&self, text: &str) -> bool {
        if let Some(prefix) = &self.prefix {
            let xargs = format!("xargs {prefix}");
            return [prefix.as_str(), xargs.as_str()]
                .iter()
                .any(|head| text == *head || text.starts_with(&format!("{head} ")));
        }
        if self.optional_tail {
            let head = self.parts[0].trim_end_matches(' ');
            if text == head {
                return true;
            }
        }
        fits(&self.parts, text)
    }
}

/// `command` as `hQ` compares it: spaces squeezed ([`squeeze`]), folded when `insensitive`.
pub(super) fn prepare(command: &str, insensitive: bool) -> String {
    let command = squeeze(command);
    if insensitive {
        command.to_lowercase()
    } else {
        command
    }
}

/// `text` with each run of spaces and tabs made one space, as `hQ` does to both sides.
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = false;
    for c in text.chars() {
        if c == ' ' || c == '\t' {
            if !blank {
                out.push(' ');
            }
            blank = true;
        } else {
            out.push(c);
            blank = false;
        }
    }
    out
}

/// Whether `parts` (literal texts with any text between them) match all of `text`.
pub(super) fn fits(parts: &[String], text: &str) -> bool {
    let Some((first, rest)) = parts.split_first() else {
        return false;
    };
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

/// Whether `rule` matches `command` as Claude Code 2.1.293 reads it ([`Rule`]).
pub(super) fn matches(rule: &str, command: &str) -> bool {
    Rule::new(rule).is_some_and(|compiled| compiled.hits(compiled.tool(), command))
}

/// Whether any of `rules` for `tool` matches `command`.
pub(super) fn denied_by(rules: &[String], tool: &str, command: &str) -> bool {
    super::index_tests::cached(rules).denies(tool, command)
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
        r"type ai.clauseo.desktop\run\own/../../settings.json",
        // A pipe, a list or a redirection straight after the folder (stage C's `|head`).
        r#"grep -r "" C:/Users/Asha/AppData/Local/ai.clauseo.desktop|head"#,
        r"ls C:\Users\Asha\AppData\Local\ai.clauseo.desktop;echo",
        r"dir C:\Users\Asha\AppData\Local\ai.clauseo.desktop>x.txt",
        // Mixed separators inside a target's path (stage C's H1).
        r"type %LOCALAPPDATA%\ai.clauseo.desktop\agents/claude-acp/home/.credentials.json",
        r"type C:\Users\Asha\AppData\Local\ai.clauseo.desktop\agents/codex-acp\home\auth.json",
        // What every session shares: the shell may not name it (stage C's H2).
        r"type C:\Users\Asha\AppData\Local\ai.clauseo.desktop\tools\x.txt",
        "ls C:/Users/Asha/AppData/Local/ai.clauseo.desktop/profile/claude-plugin/skills",
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
        // The escaped form's space is `\ `, a form no rule spells: that one is the residual.
        if command.contains(r"Firm\ Data") {
            assert!(!denied_by(&rules, "Bash", command), "{command}");
            continue;
        }
        // Each form is in the rules of the shells that can write it: Git Bash's `/c/…` and
        // `$VAR` Bash's, `$env:VAR` PowerShell's.
        let tools: &[&str] = if command.contains("$env:") {
            &["PowerShell"]
        } else if command.contains("/c/") || command.contains("$APPDATA") || command.contains("${")
        {
            &["Bash"]
        } else {
            &["Bash", "PowerShell"]
        };
        for tool in tools {
            assert!(denied_by(&rules, tool, command), "{tool}: {command}");
        }
    }
    assert!(!rules.iter().any(|r| r.contains("ai.clauseo.desktop")));
    assert!(!rules.iter().any(|r| r.contains("LOCALAPPDATA")));
}

#[test]
fn a_share_path_and_a_star_are_escaped() {
    let fixed = |path: &str| LaunchDeny::fixed(vec![PathBuf::from(path)]);
    // Written for Claude Code's parser and matcher: a `\` before a letter as itself, a run of k
    // backslashes the matcher reads as 2k − 1 (`render`).
    let rules = windows_shell_deny_rules(&fixed(r"\\?\C:\d"), &|_| None);
    assert!(
        rules.contains(&r"Bash(*C:\d\settings.json*)".to_owned()),
        "{rules:#?}"
    );
    let rules = windows_shell_deny_rules(&fixed(r"\\srv\share\d"), &|_| None);
    assert!(
        rules.contains(&r"Bash(*\\\\\srv\share\d\settings.json*)".to_owned()),
        "{rules:#?}"
    );
    assert!(denied_by(
        &rules,
        "Bash",
        r"type \\srv\share\d\settings.json"
    ));
    assert!(!denied_by(
        &rules,
        "Bash",
        r"type \srv\share\d\settings.json"
    ));
    // A target's pattern `*` stays Claude's `*`; a name's escaped one is a literal `\*`.
    let mut deny = fixed(r"C:\d");
    deny.targets = vec![r"made\*later.txt".to_owned()];
    let rules = windows_shell_deny_rules(&deny, &|_| None);
    assert!(
        rules.contains(&r"Bash(*C:\d\made\*later.txt*)".to_owned()),
        "{rules:#?}"
    );
    assert!(denied_by(&rules, "Bash", r"type C:\d\made*later.txt"));
    assert!(!denied_by(&rules, "Bash", r"type C:\d\made-or-later.txt"));
    let deny = fixed(r"C:\d");
    let rules = windows_shell_deny_rules(&deny, &|_| None);
    assert!(
        rules.contains(&r"Bash(*C:\d\state.sqlite*)".to_owned()),
        "{rules:#?}"
    );
    assert!(
        rules.contains(&r"Bash(*C:\d\run*\relay-token*)".to_owned()),
        "{rules:#?}"
    );
    assert!(!denied_by(
        &rules,
        "Bash",
        r"type C:\d\run\own\work\reply.md"
    ));
    // No rule holds a globstar (`/**/`), which Claude Code reads otherwise.
    assert!(
        !rules.iter().any(|rule| rule.contains("/**/")),
        "{rules:#?}"
    );
}

/// The port of Claude Code's reading (`Rule`) on the cases checked against the pinned binary
/// (`scripts/claude-rule-probe`, PR #1035): the settings rule as written, the command, denied.
#[test]
fn the_port_reads_rules_as_the_binary_does() {
    for (rule, command, denied) in [
        // A `\` before a letter is itself.
        (r"Bash(*C:\Users*)", r"cat C:\Users\x", true),
        (r"Bash(*C:\Users*)", "cat C:Users", false),
        // `\\` is one `\` to the parser: `\\*` reads as a literal `*`.
        (r"Bash(*abc\\*)", r"cat abc\x", false),
        (r"Bash(*ghi\\jkl*)", r"cat ghi\jkl", true),
        // `\*` is a literal `*`.
        (r"Bash(*mno\**)", "cat mno*x", true),
        (r"Bash(*mno\**)", r"cat mno\x", false),
        // `\\\*` is `\\*` to the matcher: a backslash, then any text.
        (r"Bash(*abc\\\*)", r"cat abc\x", true),
        (r"Bash(*abc\\\*)", "cat abcx", false),
        // `\\\**` (#1035's blanket bug) is a backslash, then any text.
        (r"Bash(*def\\\**)", r"cat def\x", true),
        // Five before a `*`: a backslash, then a literal `*`.
        (r"Bash(*desktop\\\\\**)", r"cat desktop\*x", true),
        (r"Bash(*desktop\\\\\**)", r"cat desktop\x", false),
        (r"Bash(*pqr\\\\\stu*)", r"cat pqr\\stu", true),
        (r"Bash(*pqr\\\\\stu*)", r"cat pqr\stu", false),
        // PowerShell rules match in any case, Bash rules as written.
        (r"PowerShell(*Desktop\x*)", r"Get-Content desktop\X", true),
        (r"Bash(*Desktop\x*)", r"cat desktop\X", false),
        // A content ending in one backslash escapes the closing `)`: the loader skips the rule.
        (r"Bash(*xyz\)", r"cat xyz\", false),
        // Two: the `)` closes it, and the parser's one backslash ends the command. In PowerShell:
        // Bash matches its parse of a command, which drops an unquoted trailing backslash.
        (r"PowerShell(*xyz\\)", r"Remove-Item xyz\", true),
        (r"PowerShell(*xyz\\)", r"Remove-Item xyz\a", false),
        // A content ending in `:*` is a legacy prefix rule, which no command starts with; `:**`
        // matches a `:` and anything.
        (r"Bash(*yyy:*)", "cat yyy:1", false),
        (r"Bash(*yyy:**)", "cat yyy:1", true),
        (r"Bash(*yyy:**)", "cat yyy:", true),
        (r"Bash(*yyy:**)", "cat yyyx", false),
        (r"Bash(git:*)", "git status", true),
    ] {
        assert_eq!(matches(rule, command), denied, "{rule} on {command}");
    }
    assert_eq!(parse(r"Bash(*xyz\)"), Err("malformed"));
    assert_eq!(parse(r"Bash(*a\(b(*)"), Err("ambiguous"));
    assert!(parse(r"Bash(*a\(b*)").is_ok());
    assert!(parse(r"Bash(*a\\(b(*)").is_ok());
    assert!(parse(r"Bash(*a(b*)").is_ok());
    assert_eq!(parse("Read(//c/x:*)"), Err("a :* the tool refuses"));
}

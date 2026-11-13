//! One probe per rule shape, for the pinned binary (`scripts/claude-rule-probe`): a rule that
//! Claude Code loads but never matches (a content ending in `:*`, say) must show up as a probe
//! the binary lets through. Every Bash rule of two laptop-like launches (the default data
//! folder, and a configured one under `%APPDATA%` with every lead form) gets a shape: how the
//! data folder is written (its name alone, a tail, its path with `\`, `/` or `\\`, `/c/…`,
//! `%VAR%`, `$env:VAR`, `$VAR`, `${VAR}`, `~`), what it names (the data folder alone, a
//! top-level target, `run`, `agent-state`, Claude's home or projects, `launch-settings`, …) and
//! the rule's form with names made `N` (each boundary character, separator and glob kept). Up to
//! [`PER_SHAPE`] rules of each shape get a command: `cat` and the rule's text with its `*`s
//! filled, quoted so the shell reads it as one word, picked so that, by the port, no other rule
//! of the launch denies it where one can be found. Run with `PROBE_HOME` set to the laptop's
//! home (`C:\Users\clauseo-lawyer`): Claude Code puts its `HOME` in place of an expandable
//! `$HOME` before it matches. The PowerShell rules no Bash rule spells
//! (`$env:VAR`) are probed through the Bash tool with the same content, as the PowerShell tool
//! does not run off Windows.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use super::tests::Rule;
use super::windows_shell_deny_rules;
use crate::agent::data_fence::big_folder::laptop;
use crate::agent::data_fence::{LaunchDeny, claude_allow_rules, claude_deny_rules_on};
use crate::pipeline::tests::support::TempDir;

/// How many rules of each shape get a probe.
const PER_SHAPE: usize = 3;

/// The laptop's default data folder.
const DATA: &str = r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop";

/// A configured data folder under `%APPDATA%`.
const CONFIGURED: &str = r"C:\Users\clauseo-lawyer\AppData\Roaming\Clauseo Data";

/// The laptop's variables.
fn env(name: &str) -> Option<OsString> {
    let home = r"C:\Users\clauseo-lawyer";
    match name {
        "LOCALAPPDATA" => Some(format!(r"{home}\AppData\Local").into()),
        "APPDATA" => Some(format!(r"{home}\AppData\Roaming").into()),
        "USERPROFILE" | "HOME" => Some(home.into()),
        _ => None,
    }
}

/// How a rule's text (its parts joined by `*`) writes the data folder, and the rest after it.
fn lead(text: &str) -> (String, &str) {
    let tails = ["agent-state", "claude-acp", "codex-acp", "run"];
    if let Some(rest) = text.strip_prefix(clauseo_settings::IDENTIFIER) {
        return ("name".to_owned(), rest);
    }
    if let Some(word) = tails.iter().find(|word| text.starts_with(*word)) {
        return (format!("tail {word}"), text);
    }
    let name = "Clauseo Data";
    let Some(at) = text.find(name).or_else(|| text.find("clauseo data")) else {
        return ("other".to_owned(), text);
    };
    let (head, rest) = text.split_at(at + name.len());
    let kind = if head.starts_with("C:\\\\") {
        "path \\\\"
    } else if head.starts_with("C:\\") && head.contains('/') {
        "$HOME as Claude Code expands it"
    } else if head.starts_with("C:\\") {
        "path \\"
    } else if head.starts_with("C:/") {
        "path /"
    } else if head.starts_with("/c/") {
        "/c/"
    } else if head.starts_with('%') && head.chars().nth(1).is_some_and(char::is_uppercase) {
        "%VAR%"
    } else if head.starts_with('%') {
        "%var%"
    } else if head.starts_with("$env:") {
        "$env:VAR"
    } else if head.starts_with("${") {
        "${VAR}"
    } else if head.starts_with('$') {
        "$VAR"
    } else if head.starts_with('~') {
        "~"
    } else {
        "other"
    };
    let var = head
        .trim_start_matches(['%', '$', '{'])
        .trim_start_matches("env:")
        .split(['%', '}', '/', '\\'])
        .next()
        .filter(|_| kind.contains("VAR") || kind.contains("var"))
        .unwrap_or("");
    (format!("{kind}{var}"), rest)
}

/// What a rule's rest names.
fn names(rest: &str) -> &'static str {
    let has = |word: &str| rest.contains(word);
    if has("launch-settings") {
        "launch-settings"
    } else if has("claude-acp") && has("projects") {
        "projects"
    } else if has("claude-acp") && has("home") {
        "home"
    } else if has("claude-acp") {
        "claude-acp"
    } else if has("agent-state") {
        "agent-state"
    } else if has("codex-acp") {
        "codex-acp"
    } else if has("runtime") {
        "runtime"
    } else if rest.starts_with("run") || has("\\run") || has("/run") {
        "run"
    } else if has("..") {
        "dots"
    } else if !rest.chars().any(char::is_alphanumeric) {
        "the folder alone"
    } else {
        "a top-level target"
    }
}

/// A rule's rest as a form: what comes before its first name, the separators between its names
/// (each kind once), and what comes after its last name, names made `N`.
fn form(rest: &str) -> String {
    let mut marked = String::new();
    for c in rest.chars() {
        let name = c.is_alphanumeric() || matches!(c, '-' | '_') || !c.is_ascii();
        if name {
            if !marked.ends_with('N') {
                marked.push('N');
            }
            continue;
        }
        match c {
            '\t' => marked.push_str("\\t"),
            '\n' => marked.push_str("\\n"),
            _ => marked.push(c),
        }
    }
    while marked.contains("N.N") {
        marked = marked.replace("N.N", "N");
    }
    let (Some(first), Some(last)) = (marked.find('N'), marked.rfind('N')) else {
        return marked;
    };
    let mut between: Vec<&str> = marked
        .get(first..=last)
        .unwrap_or_default()
        .split('N')
        .filter(|sep| !sep.is_empty())
        .collect();
    between.sort_unstable();
    between.dedup();
    format!(
        "{}N[{}]{}",
        marked.get(..first).unwrap_or_default(),
        between.join(" "),
        marked.get(last + 1..).unwrap_or_default()
    )
}

/// A rule's shape (the module doc).
fn shape(rule: &Rule) -> String {
    let text = rule.parts().get(1..).unwrap_or_default().join("*");
    let (lead, rest) = lead(&text);
    format!("{lead} | {} | {}", names(rest), form(rest))
}

/// Commands `rule` denies, in the order tried: its text with its `*`s filled, quoted.
fn candidates(rule: &Rule) -> Vec<String> {
    let parts = rule.parts();
    let open = parts.last().is_some_and(String::is_empty);
    let inner = parts
        .get(1..parts.len().saturating_sub(usize::from(open)))
        .unwrap_or_default();
    let mut out = Vec::new();
    for fill in ["", "x"] {
        let core = inner.join(fill);
        let tails: &[&str] = if open { &["x", ""] } else { &[""] };
        for tail in tails {
            let text = format!("{core}{tail}");
            if open && !text.contains('\'') {
                out.push(format!("cat '{text}'"));
            }
            if open && !text.contains('"') {
                out.push(format!("cat \"{text}\""));
            }
            out.push(format!("cat {text}"));
        }
    }
    out
}

/// The probes of a launch's `rules` for `tool`: per shape, up to [`PER_SHAPE`] of its rules each
/// with a command it denies (`name`, command, whether no other rule denies it).
fn probes(rules: &[String], tool: &str, label: &str) -> Vec<(String, String, bool)> {
    let compiled: Vec<Rule> = rules.iter().filter_map(|rule| Rule::new(rule)).collect();
    assert_eq!(compiled.len(), rules.len());
    let mut shapes: BTreeMap<String, Vec<&Rule>> = BTreeMap::new();
    for rule in compiled.iter().filter(|rule| rule.tool() == tool) {
        shapes.entry(shape(rule)).or_default().push(rule);
    }
    let mut out = Vec::new();
    for (shape, members) in &shapes {
        for rule in members.iter().take(PER_SHAPE) {
            let hits: Vec<String> = candidates(rule)
                .into_iter()
                .filter(|command| rule.hits(tool, command))
                .collect();
            assert!(!hits.is_empty(), "{shape}: no command matches its rule");
            let alone = |command: &String| {
                compiled
                    .iter()
                    .filter(|other| other.hits(tool, command))
                    .count()
                    == 1
            };
            let unique = hits.iter().find(|command| alone(command));
            let (command, unique) = unique.map_or((&hits[0], false), |command| (command, true));
            out.push((format!("{label} | {shape}"), command.clone(), unique));
        }
    }
    out
}

/// The launches probed: the default data folder and a configured one, each with its whole
/// permission settings and its Bash rules' probes, and the PowerShell-only rules' probes.
#[test]
fn every_rule_shape_has_a_probe_its_rule_denies() {
    let dir = TempDir::new("shell-deny-shapes");
    let (deny, _, _) = laptop(dir.path(), 1, 60, "clauseo-lawyer");
    let out = std::env::var_os("CLAUSEO_RULE_SHAPES_OUT").map(PathBuf::from);
    let mut total = (0, 0);
    for (label, spelling) in [("default", DATA), ("configured", CONFIGURED)] {
        let deny = LaunchDeny {
            spellings: vec![PathBuf::from(spelling)],
            ..deny.clone()
        };
        let shell = windows_shell_deny_rules(&deny, &env);
        let mut every = claude_deny_rules_on(&deny, true);
        every.extend(shell.iter().cloned());
        let allow = claude_allow_rules(&deny);
        let bash = probes(&shell, "Bash", label);
        // The PowerShell rules no Bash rule spells, as Bash rules of the same content.
        let bash_contents: std::collections::HashSet<&str> = shell
            .iter()
            .filter_map(|rule| rule.strip_prefix("Bash("))
            .collect();
        let only: Vec<String> = shell
            .iter()
            .filter_map(|rule| rule.strip_prefix("PowerShell("))
            .filter(|content| !bash_contents.contains(content))
            .map(|content| format!("Bash({content}"))
            .collect();
        let powershell = probes(&only, "Bash", &format!("{label} PowerShell-only"));
        let shapes = |probes: &[(String, String, bool)]| {
            probes
                .iter()
                .map(|(name, _, _)| name)
                .collect::<std::collections::HashSet<_>>()
                .len()
        };
        eprintln!(
            "{label}: {} Bash rules, {} shapes, {} probes ({} with no other rule); {} \
             PowerShell-only rules, {} shapes, {} probes",
            shell
                .iter()
                .filter(|rule| rule.starts_with("Bash("))
                .count(),
            shapes(&bash),
            bash.len(),
            bash.iter().filter(|(_, _, unique)| *unique).count(),
            only.len(),
            shapes(&powershell),
            powershell.len(),
        );
        total.0 += shapes(&bash) + shapes(&powershell);
        total.1 += bash.len() + powershell.len();
        let Some(out) = &out else {
            continue;
        };
        let cases = |probes: &[(String, String, bool)]| -> Vec<serde_json::Value> {
            probes
                .iter()
                .map(|(name, command, unique)| {
                    let name = if *unique {
                        name.clone()
                    } else {
                        format!("{name} (shared)")
                    };
                    // Unquoted, a `$HOME` path with a space is split by the shell and names no
                    // data folder; Claude Code, having put its `HOME` in, matches the command as
                    // it rebuilds it from those words. Reported, not checked.
                    let split = command.contains("$HOME")
                        && !command.contains(['\'', '"'])
                        && command.trim_start_matches("cat ").contains(' ');
                    let denied = if split {
                        serde_json::Value::Null
                    } else {
                        true.into()
                    };
                    serde_json::json!({"name": name, "tool": "Bash", "command": command,
                        "denied": denied})
                })
                .collect()
        };
        let settings = serde_json::json!({"permissions": {"deny": every, "allow": allow}});
        let spec = serde_json::json!({"settings": settings, "expect_skipped": [],
            "cases": cases(&bash)});
        let path = out.join(format!("{label}.json"));
        crate::write::write_file(&path, spec.to_string().as_bytes()).unwrap();
        let settings = serde_json::json!({"permissions": {"deny": only}});
        let spec = serde_json::json!({"settings": settings, "expect_skipped": [],
            "cases": cases(&powershell)});
        let path = out.join(format!("{label}-powershell.json"));
        crate::write::write_file(&path, spec.to_string().as_bytes()).unwrap();
    }
    eprintln!("shapes {}, probes {}", total.0, total.1);
    assert!(total.0 > 50);
}

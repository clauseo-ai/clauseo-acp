//! The repo-policy steps: licences, advisories and bans (cargo-deny), unused dependencies
//! (cargo-shear), spelling (typos), TOML formatting (taplo) and secrets in this branch's commits
//! (gitleaks). Each builder says what one tool runs and how its output is read. The tools are
//! pinned in `setup.rs`.

use super::ToolStep;
use super::plan::{parse_none, setup_or, setup_or_reproduce};
use crate::fix::Fix;
use crate::parse::{rel_path, short_diags};
use crate::process::{Cmd, Program};
use crate::violation::{Finding, Violation};
use crate::workspace::Context;

pub(super) fn deny_step() -> ToolStep {
    ToolStep {
        name: "deny",
        cmd: Cmd::new(Program::Cargo, ["deny", "--locked", "check"]),
        parse: parse_none,
        fix: deny_fix,
        render: None,
    }
}

fn deny_fix(cmd: &Cmd, output: Option<&str>) -> String {
    setup_or(
        output,
        format!(
            "run `{cmd}`; when it names a dependency, replace or drop it and never loosen deny.toml \
             to pass, and when it prints a cargo error instead, fix that error first"
        ),
    )
}

pub(super) fn shear_step() -> ToolStep {
    ToolStep {
        name: "shear",
        cmd: Cmd::new(Program::Cargo, ["shear", "--deny-warnings"]),
        parse: parse_none,
        fix: shear_fix,
        render: None,
    }
}

fn shear_fix(_cmd: &Cmd, output: Option<&str>) -> String {
    setup_or(
        output,
        "when cargo shear names an unused dependency, remove it from its Cargo.toml (`cargo shear \
         --fix` does it), and when it prints a cargo error instead, fix that error first"
            .into(),
    )
}

pub(super) fn typos_step() -> ToolStep {
    ToolStep {
        name: "typos",
        cmd: Cmd::new(Program::Named("typos"), ["--format", "brief"]),
        parse: typos_findings,
        fix: setup_or_reproduce,
        render: None,
    }
}

/// The `path:line:col: error: ...` lines from `typos --format brief`.
fn typos_findings(output: &str, ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    short_diags(output, &ctx.root.to_string_lossy())
        .into_iter()
        .map(|diag| Finding {
            violation: Violation {
                path: diag.path,
                line: diag.line,
                message: diag
                    .message
                    .strip_prefix("error: ")
                    .unwrap_or(&diag.message)
                    .to_owned(),
                fix: Fix::tool(
                    "correct the spelling (`typos -w` applies the suggestion); a real name or \
                     term goes under [default.extend-words] in _typos.toml",
                ),
            },
            is_error: true,
        })
        .collect()
}

pub(super) fn taplo_step() -> ToolStep {
    ToolStep {
        name: "taplo",
        cmd: Cmd::new(Program::Named("taplo"), ["fmt", "--check"]),
        parse: taplo_findings,
        fix: setup_or_reproduce,
        render: None,
    }
}

/// One finding per `... the file is not properly formatted path="<path>"` line. taplo prints the
/// path in Rust's debug form, so a Windows path arrives as `C:\\ws\\deny.toml`.
fn taplo_findings(output: &str, ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    const MARKER: &str = "not properly formatted path=\"";
    output
        .lines()
        .filter_map(|line| {
            let start = line.find(MARKER)? + MARKER.len();
            let raw = debug_string(line.get(start..)?)?;
            let path = repo_path(&raw, ctx);
            Some(Finding {
                violation: Violation {
                    message: "taplo would reformat this file".into(),
                    fix: Fix::tool(format!("run `taplo fmt {path}`")),
                    path,
                    line: 1,
                },
                is_error: true,
            })
        })
        .collect()
}

/// The text of a Rust debug-form string up to its closing quote: `\\` and `\"` are unescaped.
/// `None` when the closing quote is missing.
fn debug_string(after_quote: &str) -> Option<String> {
    let mut text = String::new();
    let mut chars = after_quote.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(text),
            '\\' => text.push(chars.next()?),
            _ => text.push(c),
        }
    }
    None
}

/// Repo-relative form of an absolute path a tool printed. Tries the root as given and its
/// canonical form, since a tool may print either (`/tmp` and `/private/tmp` on macOS).
fn repo_path(raw: &str, ctx: &Context) -> String {
    let mut roots = vec![ctx.root.to_string_lossy().into_owned()];
    if let Ok(canonical) = dunce::canonicalize(&ctx.root) {
        roots.push(canonical.to_string_lossy().into_owned());
    }
    relative_to_any(raw, &roots)
}

/// `raw` relative to the first of `roots` it lies under, `/`-separated, or all of `raw` when it
/// lies under none. Windows verbatim prefixes (`\\?\`, which `dunce::canonicalize` keeps on a path
/// it cannot write without one, and a tool may print) are dropped from both sides first.
fn relative_to_any(raw: &str, roots: &[String]) -> String {
    let raw = strip_verbatim(raw);
    let whole = raw.trim().replace('\\', "/");
    roots
        .iter()
        .map(|root| rel_path(raw, strip_verbatim(root)))
        .find(|rel| *rel != whole)
        .unwrap_or(whole)
}

fn strip_verbatim(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

/// gitleaks over the commits in `range` (a git range such as `<base>..HEAD`), or over all history
/// without one.
pub(super) fn gitleaks_step(range: Option<&str>) -> ToolStep {
    let mut args = vec![
        "git".to_owned(),
        "--redact".to_owned(),
        "--no-banner".to_owned(),
        "--verbose".to_owned(),
        "--no-color".to_owned(),
    ];
    if let Some(range) = range {
        args.push(format!("--log-opts={range}"));
    }
    ToolStep {
        name: "gitleaks",
        cmd: Cmd::new(Program::Named("gitleaks"), args),
        parse: gitleaks_findings,
        fix: setup_or_reproduce,
        render: None,
    }
}

/// One finding per leak block of `gitleaks --verbose` (`RuleID:`, `File:`, `Line:`, `Commit:`).
fn gitleaks_findings(output: &str, _ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut leak = Leak::default();
    for line in strip_ansi(output).lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Finding" => {
                findings.extend(leak.finding());
                leak = Leak::default();
            }
            "RuleID" => leak.rule = Some(value.to_owned()),
            "File" => leak.file = Some(value.replace('\\', "/")),
            "Line" => leak.line = value.parse().ok(),
            "Commit" => leak.commit = Some(value.to_owned()),
            _ => {}
        }
    }
    findings.extend(leak.finding());
    findings
}

#[derive(Default)]
struct Leak {
    rule: Option<String>,
    file: Option<String>,
    line: Option<u32>,
    commit: Option<String>,
}

impl Leak {
    fn finding(&self) -> Option<Finding> {
        let commit = self.commit.as_deref()?;
        let short = commit.get(..12).unwrap_or(commit);
        Some(Finding {
            violation: Violation {
                path: self.file.clone()?,
                line: self.line?,
                message: format!(
                    "gitleaks rule {} matched a secret in commit {short}",
                    self.rule.as_deref().unwrap_or("unknown")
                ),
                fix: Fix::tool(
                    "remove the secret from this branch's commits (rewrite them) and rotate it if \
                     it is real; a fake secret in a test fixture goes in .gitleaks.toml's allowlist",
                ),
            },
            is_error: true,
        })
    }
}

/// `text` without ANSI colour sequences (`ESC [ ... letter`), which gitleaks prints even to a pipe.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn context() -> Context {
        Context {
            root: PathBuf::from("/ws"),
            members: Ok(Vec::new()),
            load_resolved_graph: false,
        }
    }

    #[test]
    fn commands_are_the_pinned_policy_invocations() {
        assert_eq!(deny_step().cmd.to_string(), "cargo deny --locked check");
        assert_eq!(shear_step().cmd.to_string(), "cargo shear --deny-warnings");
        assert_eq!(typos_step().cmd.to_string(), "typos --format brief");
        assert_eq!(taplo_step().cmd.to_string(), "taplo fmt --check");
        assert_eq!(
            gitleaks_step(Some("abc123..HEAD")).cmd.to_string(),
            "gitleaks git --redact --no-banner --verbose --no-color --log-opts=abc123..HEAD"
        );
        assert_eq!(
            gitleaks_step(None).cmd.to_string(),
            "gitleaks git --redact --no-banner --verbose --no-color"
        );
    }

    #[test]
    fn a_missing_tool_points_at_setup() {
        let cmd = deny_step().cmd;
        assert_eq!(deny_fix(&cmd, None), "run `cargo xtask setup`");
        assert_eq!(
            deny_fix(&cmd, Some("error: no such command: `deny`")),
            "run `cargo xtask setup`"
        );
        assert!(deny_fix(&cmd, Some("error[banned]: ...")).contains("never loosen deny.toml"));
        let typos = typos_step().cmd;
        assert_eq!(setup_or_reproduce(&typos, None), "run `cargo xtask setup`");
        assert_eq!(
            setup_or_reproduce(&typos, Some("boom")),
            "run `typos --format brief` to reproduce"
        );
    }

    #[test]
    fn typos_findings_carry_file_line_and_the_suggestion() {
        // The misspelling is split so that typos does not flag this file.
        let word = concat!("t", "eh");
        let out = format!("./xtask/src/main.rs:3:8: error: `{word}` should be `the`\n");
        let findings = typos_findings(&out, &context(), &typos_step().cmd);
        assert_eq!(findings.len(), 1);
        let v = &findings[0].violation;
        assert_eq!(
            (v.path.as_str(), v.line, v.message.as_str()),
            (
                "xtask/src/main.rs",
                3,
                format!("`{word}` should be `the`").as_str()
            )
        );
        assert!(findings[0].is_error);
    }

    #[test]
    fn taplo_findings_name_each_unformatted_file() {
        let out = " INFO taplo:format_files:collect_files: found files total=2\n\
                   ERROR taplo:format_files: the file is not properly formatted path=\"/ws/deny.toml\"\n\
                   ERROR operation failed error=some files were not properly formatted\n";
        let findings = taplo_findings(out, &context(), &taplo_step().cmd);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].violation.path, "deny.toml");
        assert_eq!(findings[0].violation.line, 1);
        assert_eq!(
            findings[0].violation.fix.to_string(),
            "run `taplo fmt deny.toml`"
        );
    }

    #[test]
    fn taplo_findings_read_a_windows_path_in_debug_form() {
        let ctx = Context {
            root: PathBuf::from(r"C:\ws"),
            members: Ok(Vec::new()),
            load_resolved_graph: false,
        };
        let out = r#"ERROR taplo:format_files: the file is not properly formatted path="C:\\ws\\xtask\\Cargo.toml""#;
        let findings = taplo_findings(out, &ctx, &taplo_step().cmd);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].violation.path, "xtask/Cargo.toml");
    }

    #[test]
    fn a_path_is_made_relative_to_a_verbatim_canonical_root() {
        let roots = [r"C:\link".to_owned(), r"\\?\C:\ws".to_owned()];
        assert_eq!(relative_to_any(r"C:\ws\deny.toml", &roots), "deny.toml");
        assert_eq!(relative_to_any(r"\\?\C:\ws\a\b.toml", &roots), "a/b.toml");
        assert_eq!(
            relative_to_any("/elsewhere/x.toml", &roots),
            "/elsewhere/x.toml"
        );
    }

    #[test]
    fn debug_strings_unescape_backslashes_and_quotes() {
        assert_eq!(
            debug_string(r#"C:\\a\"b" rest"#).as_deref(),
            Some(r#"C:\a"b"#)
        );
        assert_eq!(debug_string("no closing quote"), None);
    }

    #[test]
    fn gitleaks_findings_read_each_verbose_block() {
        let out = "Finding:     \u{1b}[1;3;mREDACTED\u{1b}[0m\n\
                   Secret:      REDACTED\n\
                   RuleID:      private-key\n\
                   File:        dir/key.pem\n\
                   Line:        1\n\
                   Commit:      e1ac725f63594b7ee93debd00cd2bca14e929703\n\
                   \n\
                   Finding:     REDACTED\n\
                   RuleID:      generic-api-key\n\
                   File:        src\\a.rs\n\
                   Line:        7\n\
                   Commit:      0123456789abcdef\n\
                   \n\
                   \u{1b}[90m4:20PM\u{1b}[0m \u{1b}[33mWRN\u{1b}[0m leaks found: 2\n";
        let findings = gitleaks_findings(out, &context(), &gitleaks_step(None).cmd);
        let found: Vec<String> = findings
            .iter()
            .map(|f| {
                format!(
                    "{}:{}: {}",
                    f.violation.path, f.violation.line, f.violation.message
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                "dir/key.pem:1: gitleaks rule private-key matched a secret in commit e1ac725f6359",
                "src/a.rs:7: gitleaks rule generic-api-key matched a secret in commit 0123456789ab",
            ]
        );
    }

    #[test]
    fn gitleaks_finds_nothing_in_a_clean_run() {
        let out = "\u{1b}[90m4:15PM\u{1b}[0m \u{1b}[32mINF\u{1b}[0m 0 commits scanned.\n\
                   4:15PM INF no leaks found\n";
        assert!(gitleaks_findings(out, &context(), &gitleaks_step(None).cmd).is_empty());
    }
}

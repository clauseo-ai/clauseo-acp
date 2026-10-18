//! The repo-policy steps: licences, advisories and bans (cargo-deny), unused dependencies
//! (cargo-shear), spelling (typos), TOML formatting (taplo) and secrets in this branch's commits
//! (gitleaks). Each builder says what one tool runs and how its output is read. The tools are pinned
//! in `setup.rs`.

use super::ToolStep;
use super::plan::{parse_none, reproduce};
use crate::parse::{clippy_diags, rel_path};
use crate::process::{Cmd, Program};
use crate::violation::{Finding, Violation};
use crate::workspace::Context;

/// Points at `cargo xtask setup` when the tool is missing: it could not start, or cargo has no
/// such subcommand. Otherwise `otherwise`.
fn setup_or(output: Option<&str>, otherwise: String) -> String {
    match output {
        None => "run `cargo xtask setup`".into(),
        Some(text) if text.contains("no such command") => "run `cargo xtask setup`".into(),
        Some(_) => otherwise,
    }
}

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
            "run `{cmd}`, then replace or drop the dependency it names; never loosen deny.toml to pass"
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
        "remove the unused dependency from its Cargo.toml (`cargo shear --fix` does it)".into(),
    )
}

pub(super) fn typos_step() -> ToolStep {
    ToolStep {
        name: "typos",
        cmd: Cmd::new(Program::Named("typos"), ["--format", "brief"]),
        parse: typos_findings,
        fix: tool_fix,
        render: None,
    }
}

/// The `path:line:col: error: ...` lines from `typos --format brief`.
fn typos_findings(output: &str, ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    clippy_diags(output, &ctx.root.to_string_lossy())
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
                fix: "correct the spelling (`typos -w` applies the suggestion); a real name or \
                      term goes under [default.extend-words] in _typos.toml"
                    .into(),
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
        fix: tool_fix,
        render: None,
    }
}

/// One finding per `... the file is not properly formatted path="<path>"` line.
fn taplo_findings(output: &str, ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    const MARKER: &str = "not properly formatted path=\"";
    output
        .lines()
        .filter_map(|line| {
            let start = line.find(MARKER)? + MARKER.len();
            let raw = line.get(start..)?.split('"').next()?;
            let path = repo_path(raw, ctx);
            Some(Finding {
                violation: Violation {
                    message: "taplo would reformat this file".into(),
                    fix: format!("run `taplo fmt {path}`"),
                    path,
                    line: 1,
                },
                is_error: true,
            })
        })
        .collect()
}

/// Repo-relative form of an absolute path a tool printed. Tries the root as given and its
/// canonical form, since a tool may print either (`/tmp` and `/private/tmp` on macOS).
fn repo_path(raw: &str, ctx: &Context) -> String {
    let given = ctx.root.to_string_lossy();
    let path = rel_path(raw, &given);
    if path != raw.trim().replace('\\', "/") {
        return path;
    }
    match std::fs::canonicalize(&ctx.root) {
        Ok(canonical) => rel_path(raw, &canonical.to_string_lossy()),
        Err(_) => path,
    }
}

/// gitleaks over the commits after `base` (`<base>..HEAD`), or over all history without a base.
pub(super) fn gitleaks_step(base: Option<&str>) -> ToolStep {
    let mut args = vec![
        "git".to_owned(),
        "--redact".to_owned(),
        "--no-banner".to_owned(),
        "--verbose".to_owned(),
        "--no-color".to_owned(),
    ];
    if let Some(base) = base {
        args.push(format!("--log-opts={base}..HEAD"));
    }
    ToolStep {
        name: "gitleaks",
        cmd: Cmd::new(Program::Named("gitleaks"), args),
        parse: gitleaks_findings,
        fix: tool_fix,
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
                fix: "remove the secret from this branch's commits (rewrite them) and rotate it if \
                      it is real; a fake secret in a test fixture goes in .gitleaks.toml's allowlist"
                    .into(),
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

/// For a program on `PATH`: setup when it cannot start, else the command that reproduces.
fn tool_fix(cmd: &Cmd, output: Option<&str>) -> String {
    setup_or(output, reproduce(cmd, output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn context() -> Context {
        Context {
            root: PathBuf::from("/ws"),
            members: Vec::new(),
        }
    }

    #[test]
    fn commands_are_the_pinned_policy_invocations() {
        assert_eq!(deny_step().cmd.to_string(), "cargo deny --locked check");
        assert_eq!(shear_step().cmd.to_string(), "cargo shear --deny-warnings");
        assert_eq!(typos_step().cmd.to_string(), "typos --format brief");
        assert_eq!(taplo_step().cmd.to_string(), "taplo fmt --check");
        assert_eq!(
            gitleaks_step(Some("abc123")).cmd.to_string(),
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
        assert_eq!(tool_fix(&typos, None), "run `cargo xtask setup`");
        assert_eq!(
            tool_fix(&typos, Some("boom")),
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
        assert_eq!(findings[0].violation.fix, "run `taplo fmt deny.toml`");
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

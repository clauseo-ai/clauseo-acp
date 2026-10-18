//! The external steps: `fmt`, `clippy` and `nextest`. Each returns an `Outcome` for the orchestrator to print.

use crate::parse;
use crate::violation::Violation;
use crate::workspace::{Context, Member, package_for};
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;

/// Which packages a cargo step covers.
#[derive(Debug, PartialEq, Eq)]
pub enum Scope {
    Workspace,
    Packages(Vec<String>),
}

impl Scope {
    fn selectors(&self) -> Vec<String> {
        match self {
            Scope::Workspace => vec!["--workspace".into()],
            Scope::Packages(names) => names
                .iter()
                .flat_map(|name| ["-p".to_string(), name.clone()])
                .collect(),
        }
    }
}

/// Output printed under a failure when nothing could be parsed: the tool's output, then a `fix:` line.
#[derive(Debug)]
pub struct Raw {
    pub output: String,
    pub fix: String,
}

/// What one step found.
#[derive(Debug, Default)]
pub struct Outcome {
    pub failed: bool,
    pub warnings: usize,
    pub violations: Vec<Violation>,
    pub raw: Option<Raw>,
}

/// Exit status and merged stdout/stderr of a cargo command.
pub struct Captured {
    pub success: bool,
    pub output: String,
}

/// Runs `cargo <args>` in `root`, with stdout and stderr merged into one stream.
pub fn capture(root: &Path, args: &[String]) -> io::Result<Captured> {
    let (mut reader, writer) = std::io::pipe()?;
    let mut command = Command::new("cargo");
    command
        .args(args)
        .current_dir(root)
        .stdout(writer.try_clone()?)
        .stderr(writer);
    let mut child = command.spawn()?;
    // The command still holds the write end of the pipe; drop it so the reader sees EOF.
    drop(command);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    let status = child.wait()?;
    Ok(Captured {
        success: status.success(),
        output: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// The shell-like form of `cargo <args>`, for display.
pub fn display(args: &[String]) -> String {
    format!("cargo {}", args.join(" "))
}

fn owned(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn spawn_failure(error: io::Error) -> Outcome {
    Outcome {
        failed: true,
        violations: vec![Violation {
            path: "rust-toolchain.toml".into(),
            line: 1,
            message: format!("could not run `cargo`: {error}"),
            fix: "install Rust with rustup and put `cargo` on PATH; the pinned toolchain is in rust-toolchain.toml".into(),
        }],
        ..Outcome::default()
    }
}

fn root_str(ctx: &Context) -> String {
    ctx.root.to_string_lossy().into_owned()
}

/// `cargo fmt --all -- --check`.
pub fn fmt(ctx: &Context) -> Outcome {
    let args = owned(&["fmt", "--all", "--", "--check"]);
    let captured = match capture(&ctx.root, &args) {
        Ok(captured) => captured,
        Err(error) => return spawn_failure(error),
    };
    if captured.success {
        return Outcome::default();
    }
    let diffs = parse::fmt_diffs(&captured.output, &root_str(ctx));
    if diffs.is_empty() {
        return Outcome {
            failed: true,
            raw: Some(Raw {
                output: captured.output,
                fix: "run `cargo fmt --all -- --check` to reproduce".into(),
            }),
            ..Outcome::default()
        };
    }
    Outcome {
        failed: true,
        violations: diffs
            .into_iter()
            .map(|(path, line)| Violation {
                path,
                line,
                message: "rustfmt would reformat this code".into(),
                fix: "run `cargo fmt --all`".into(),
            })
            .collect(),
        ..Outcome::default()
    }
}

/// Arguments of the clippy step: `clippy <scope> --all-targets --message-format=short`,
/// plus `-- -D warnings` in CI.
pub fn clippy_args(scope: &Scope, ci: bool) -> Vec<String> {
    let mut args = owned(&["clippy"]);
    args.extend(scope.selectors());
    args.extend(owned(&["--all-targets", "--message-format=short"]));
    if ci {
        args.extend(owned(CI_CLIPPY_SUFFIX));
    }
    args
}

/// Appended to clippy's command in CI so warnings fail the step.
const CI_CLIPPY_SUFFIX: &[&str] = &["--", "-D", "warnings"];

/// Arguments of the nextest step: `nextest run <scope> --no-tests=pass`, plus `--profile ci` in CI.
pub fn nextest_args(scope: &Scope, ci: bool) -> Vec<String> {
    let mut args = owned(&["nextest", "run"]);
    args.extend(scope.selectors());
    args.extend(owned(&["--no-tests=pass"]));
    if ci {
        args.extend(owned(&["--profile", "ci"]));
    }
    args
}

/// Runs the clippy step (see `clippy_args`).
pub fn clippy(ctx: &Context, scope: &Scope, ci: bool) -> Outcome {
    let suffix: &[&str] = if ci { CI_CLIPPY_SUFFIX } else { &[] };
    let args = clippy_args(scope, ci);
    let captured = match capture(&ctx.root, &args) {
        Ok(captured) => captured,
        Err(error) => return spawn_failure(error),
    };

    let diags = parse::clippy_diags(&captured.output, &root_str(ctx));
    let errors = diags.iter().filter(|d| d.is_error).count();
    let warnings = diags.len() - errors;
    let violations = diags
        .into_iter()
        .map(|d| {
            let fix = clippy_fix(&d.path, &ctx.members, scope, suffix);
            Violation {
                path: d.path,
                line: d.line,
                message: d.message,
                fix,
            }
        })
        .collect();
    let failed = !captured.success;
    let raw = (failed && errors == 0).then(|| Raw {
        output: captured.output,
        fix: format!("run `{}` to reproduce", clippy_repro(scope, suffix, None)),
    });
    Outcome {
        failed,
        warnings,
        violations,
        raw,
    }
}

/// The command a developer runs to see the full clippy output for `path`.
fn clippy_fix(path: &str, members: &[Member], scope: &Scope, suffix: &[&str]) -> String {
    let package = package_for(path, members).map(|m| m.name.as_str());
    format!(
        "`{}` shows the full diagnostic and clippy's suggested change",
        clippy_repro(scope, suffix, package)
    )
}

fn clippy_repro(scope: &Scope, suffix: &[&str], package: Option<&str>) -> String {
    let selectors = match (package, scope) {
        (Some(name), _) => format!("-p {name}"),
        (None, Scope::Workspace) => "--workspace".to_string(),
        (None, Scope::Packages(_)) => scope.selectors().join(" "),
    };
    let suffix = if suffix.is_empty() {
        String::new()
    } else {
        format!(" {}", suffix.join(" "))
    };
    format!("cargo clippy {selectors} --all-targets{suffix}")
}

/// Runs the nextest step (see `nextest_args`).
pub fn nextest(ctx: &Context, scope: &Scope, ci: bool) -> Outcome {
    let args = nextest_args(scope, ci);
    let captured = match capture(&ctx.root, &args) {
        Ok(captured) => captured,
        Err(error) => return spawn_failure(error),
    };
    if captured.success {
        return Outcome::default();
    }
    let fix = if captured.output.contains("no such command: `nextest`") {
        "run `cargo xtask setup`".to_string()
    } else {
        format!("run `{}` to reproduce", display(&args))
    };
    Outcome {
        failed: true,
        raw: Some(Raw {
            output: captured.output,
            fix,
        }),
        ..Outcome::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clippy_header_matches_the_command_run_in_ci() {
        assert_eq!(
            display(&clippy_args(&Scope::Workspace, true)),
            "cargo clippy --workspace --all-targets --message-format=short -- -D warnings"
        );
        assert_eq!(
            display(&clippy_args(&Scope::Workspace, false)),
            "cargo clippy --workspace --all-targets --message-format=short"
        );
    }

    #[test]
    fn nextest_header_matches_the_command_run_in_ci() {
        assert_eq!(
            display(&nextest_args(&Scope::Workspace, true)),
            "cargo nextest run --workspace --no-tests=pass --profile ci"
        );
        assert_eq!(
            display(&nextest_args(&Scope::Workspace, false)),
            "cargo nextest run --workspace --no-tests=pass"
        );
    }

    #[test]
    fn clippy_fix_names_the_owning_package() {
        let members = vec![Member {
            name: "xtask".into(),
            dir: "xtask".into(),
            path_deps: vec![],
        }];
        let fix = clippy_fix("xtask/src/main.rs", &members, &Scope::Workspace, &[]);
        assert_eq!(
            fix,
            "`cargo clippy -p xtask --all-targets` shows the full diagnostic and clippy's suggested change"
        );
    }

    #[test]
    fn clippy_fix_falls_back_to_the_step_command_in_ci() {
        let fix = clippy_fix(
            "docs/x.md",
            &[],
            &Scope::Packages(vec!["a".into(), "b".into()]),
            &["--", "-D", "warnings"],
        );
        assert_eq!(
            fix,
            "`cargo clippy -p a -p b --all-targets -- -D warnings` shows the full diagnostic and clippy's suggested change"
        );
    }
}

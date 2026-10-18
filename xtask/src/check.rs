//! `cargo xtask check`: runs every step, prints failures, then a summary.

use crate::changes::{self, Selection};
use crate::steps::{self, Outcome, Scope};
use crate::violation::Violation;
use crate::workspace::{self, Context};
use std::process::ExitCode;
use std::time::Instant;

/// A check written in Rust rather than run as a tool. R2 to R4 add entries to `STATIC_CHECKS`.
pub struct StaticCheck {
    pub name: &'static str,
    pub run: fn(&Context) -> Vec<Violation>,
}

pub const STATIC_CHECKS: &[StaticCheck] = &[];

/// One line of the final summary.
struct StepSummary {
    name: &'static str,
    failed: bool,
    warnings: usize,
    secs: Option<f64>,
    skipped: Option<&'static str>,
}

/// Runs the whole check. `quick` limits clippy and nextest to changed packages.
pub fn run(quick: bool) -> ExitCode {
    let root = workspace::workspace_root();
    let ci = std::env::var("CI").is_ok_and(|value| !value.is_empty());
    let (members, members_error) = match workspace::load_members(&root) {
        Ok(members) => (members, None),
        Err(error) => {
            println!("note: {error}");
            (Vec::new(), Some(error))
        }
    };
    let ctx = Context { root, members };

    let scope = if quick {
        quick_scope(&ctx, members_error.as_deref())
    } else {
        Scope::Workspace
    };
    let scope_empty = matches!(&scope, Scope::Packages(names) if names.is_empty());

    let mut summaries = Vec::new();
    step(&mut summaries, "fmt", "cargo fmt --all -- --check", || {
        steps::fmt(&ctx)
    });
    for check in STATIC_CHECKS {
        step(&mut summaries, check.name, "built-in static check", || {
            let violations = (check.run)(&ctx);
            Outcome {
                failed: !violations.is_empty(),
                violations,
                ..Outcome::default()
            }
        });
    }
    if scope_empty {
        skip(&mut summaries, "clippy", "no changed packages");
        skip(&mut summaries, "nextest", "no changed packages");
    } else {
        let clippy_command = steps::display(&steps::clippy_args(&scope, ci));
        step(&mut summaries, "clippy", &clippy_command, || {
            steps::clippy(&ctx, &scope, ci)
        });
        let nextest_command = steps::display(&steps::nextest_args(&scope, ci));
        step(&mut summaries, "nextest", &nextest_command, || {
            steps::nextest(&ctx, &scope, ci)
        });
    }

    println!("summary:");
    let mut any_failed = false;
    for summary in &summaries {
        any_failed |= summary.failed;
        println!("{}", summary_line(summary));
    }
    if any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Decides what `--quick` covers, and prints the decision.
fn quick_scope(ctx: &Context, members_error: Option<&str>) -> Scope {
    if let Some(error) = members_error {
        println!("quick: all packages (cargo metadata failed: {error})");
        return Scope::Workspace;
    }
    let files = match changes::changed_files(&ctx.root) {
        Ok(files) => files,
        Err(error) => {
            println!("quick: all packages (could not diff against origin/main: {error})");
            return Scope::Workspace;
        }
    };
    match changes::select(&files, &ctx.members) {
        Selection::All(reason) => {
            println!("quick: all packages ({reason})");
            Scope::Workspace
        }
        Selection::Packages(names) if names.is_empty() => {
            println!("quick: no changed packages");
            Scope::Packages(names)
        }
        Selection::Packages(names) => {
            let direct = changes::direct_members(&files, &ctx.members);
            println!(
                "quick: packages {} (changed: {}; dependents: {})",
                names.join(", "),
                list_or_none(&direct),
                list_or_none(
                    &names
                        .iter()
                        .filter(|n| !direct.contains(n))
                        .cloned()
                        .collect::<Vec<_>>()
                ),
            );
            Scope::Packages(names)
        }
    }
}

fn list_or_none(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

/// Prints the step header, runs it, prints what it found, and records its summary line.
fn step(
    summaries: &mut Vec<StepSummary>,
    name: &'static str,
    command: &str,
    run: impl FnOnce() -> Outcome,
) {
    println!("==> {name}: {command}");
    let started = Instant::now();
    let outcome = run();
    let secs = started.elapsed().as_secs_f64();
    for violation in &outcome.violations {
        println!("{violation}");
    }
    if let Some(raw) = &outcome.raw {
        print!("{}", raw.output);
        if !raw.output.is_empty() && !raw.output.ends_with('\n') {
            println!();
        }
        println!("    fix: {}", raw.fix);
    }
    summaries.push(StepSummary {
        name,
        failed: outcome.failed,
        warnings: outcome.warnings,
        secs: Some(secs),
        skipped: None,
    });
}

fn skip(summaries: &mut Vec<StepSummary>, name: &'static str, reason: &'static str) {
    println!("==> {name}: skipped ({reason})");
    summaries.push(StepSummary {
        name,
        failed: false,
        warnings: 0,
        secs: None,
        skipped: Some(reason),
    });
}

/// `fmt ok`, `clippy FAILED (2 warnings, 3.1 s)`, `clippy ok (skipped: no changed packages)`.
fn summary_line(summary: &StepSummary) -> String {
    let status = if summary.failed { "FAILED" } else { "ok" };
    let mut parts = Vec::new();
    if let Some(reason) = summary.skipped {
        parts.push(format!("skipped: {reason}"));
    } else {
        if summary.warnings > 0 {
            let noun = if summary.warnings == 1 {
                "warning"
            } else {
                "warnings"
            };
            parts.push(format!("{} {noun}", summary.warnings));
        }
        if let Some(secs) = summary.secs {
            parts.push(format!("{secs:.1} s"));
        }
    }
    if parts.is_empty() {
        format!("{} {status}", summary.name)
    } else {
        format!("{} {status} ({})", summary.name, parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(failed: bool, warnings: usize, secs: Option<f64>) -> StepSummary {
        StepSummary {
            name: "clippy",
            failed,
            warnings,
            secs,
            skipped: None,
        }
    }

    #[test]
    fn summary_lines_match_the_documented_shape() {
        assert_eq!(
            summary_line(&summary(false, 0, Some(1.25))),
            "clippy ok (1.2 s)"
        );
        assert_eq!(
            summary_line(&summary(false, 2, Some(3.1))),
            "clippy ok (2 warnings, 3.1 s)"
        );
        assert_eq!(
            summary_line(&summary(true, 1, None)),
            "clippy FAILED (1 warning)"
        );
        let skipped = StepSummary {
            name: "nextest",
            failed: false,
            warnings: 0,
            secs: None,
            skipped: Some("no changed packages"),
        };
        assert_eq!(
            summary_line(&skipped),
            "nextest ok (skipped: no changed packages)"
        );
    }
}

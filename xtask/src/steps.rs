//! Steps as data. `plan` lists what runs and in what order, and `run_all` is the one loop that runs it.
//! Adding an external step is one `ToolStep` in `plan`.

use crate::checks::{self, StaticCheck};
use crate::output;
use crate::parse;
use crate::process::{Captured, Cmd, Program};
use crate::violation::Violation;
use crate::workspace::Context;
use std::io;
use std::time::Instant;

const NO_PACKAGES: &str = "no changed packages";

/// Which packages a cargo step covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every member of the workspace.
    Workspace,
    /// These members. Never empty: build it with `Scope::packages`.
    Packages(Vec<String>),
    /// No members. Steps that need packages are skipped.
    Nothing,
}

impl Scope {
    /// `Packages(names)`, or `Nothing` when `names` is empty. An empty list must never reach cargo,
    /// where it would mean every member.
    pub fn packages(names: Vec<String>) -> Scope {
        if names.is_empty() {
            Scope::Nothing
        } else {
            Scope::Packages(names)
        }
    }

    /// The cargo selector arguments, or `None` for `Nothing`.
    fn selectors(&self) -> Option<Vec<String>> {
        match self {
            Scope::Workspace => Some(vec!["--workspace".into()]),
            Scope::Packages(names) => Some(
                names
                    .iter()
                    .flat_map(|name| ["-p".to_string(), name.clone()])
                    .collect(),
            ),
            Scope::Nothing => None,
        }
    }
}

/// A problem a tool reported. An error fails the step; a warning is printed and counted.
pub struct Finding {
    pub violation: Violation,
    pub is_error: bool,
}

/// An external tool run as `cmd`.
pub struct ToolStep {
    pub name: &'static str,
    /// Exactly what runs, and what the header prints.
    pub cmd: Cmd,
    /// The `file:line` findings in the output. Returns none when the output has no locations.
    pub parse: fn(&str, &Context, &Cmd) -> Vec<Finding>,
    /// The fix line for a failure with no `file:line`, or for a program that cannot start.
    /// `None` means the program could not start; `Some` is the captured output.
    pub fix: fn(&Cmd, Option<&str>) -> String,
}

pub enum Step {
    Tool(ToolStep),
    Static(&'static StaticCheck),
    Skipped {
        name: &'static str,
        reason: &'static str,
    },
}

/// The summary entry for one step.
pub struct StepResult {
    pub name: String,
    pub status: StepStatus,
}

#[derive(Debug, PartialEq)]
pub enum StepStatus {
    Ran {
        failed: bool,
        warnings: usize,
        secs: f64,
    },
    Skipped(&'static str),
}

/// The steps for `scope`, in run order: fmt, every static check, clippy, nextest.
/// With `Scope::Nothing`, clippy and nextest are skipped. `ci` picks the CI variants of the
/// clippy and nextest commands.
pub fn plan(scope: &Scope, ci: bool) -> Vec<Step> {
    let mut steps = vec![Step::Tool(fmt_step())];
    steps.extend(checks::ALL.iter().map(Step::Static));
    match scope.selectors() {
        Some(selectors) => {
            steps.push(Step::Tool(clippy_step(&selectors, ci)));
            steps.push(Step::Tool(nextest_step(&selectors, ci)));
        }
        None => {
            steps.push(Step::Skipped {
                name: "clippy",
                reason: NO_PACKAGES,
            });
            steps.push(Step::Skipped {
                name: "nextest",
                reason: NO_PACKAGES,
            });
        }
    }
    steps
}

/// Runs every step in order, even after one fails. Prints each step as it goes and returns one
/// result per step. `exec` runs a tool command; production passes `process::capture`.
pub fn run_all(
    steps: Vec<Step>,
    ctx: &Context,
    exec: &mut dyn FnMut(&Cmd) -> io::Result<Captured>,
) -> Vec<StepResult> {
    let mut results = Vec::new();
    for step in steps {
        let result = match step {
            Step::Skipped { name, reason } => {
                output::header(name, format!("skipped ({reason})"));
                StepResult {
                    name: name.into(),
                    status: StepStatus::Skipped(reason),
                }
            }
            Step::Static(check) => run_static(check, ctx),
            Step::Tool(tool) => run_tool(&tool, ctx, exec),
        };
        results.push(result);
    }
    results
}

fn run_static(check: &StaticCheck, ctx: &Context) -> StepResult {
    output::header(check.name, "built-in static check");
    let (violations, secs) = timed(|| (check.run)(ctx));
    for found in &violations {
        output::violation(found);
    }
    StepResult {
        name: check.name.into(),
        status: StepStatus::Ran {
            failed: !violations.is_empty(),
            warnings: 0,
            secs,
        },
    }
}

fn run_tool(
    tool: &ToolStep,
    ctx: &Context,
    exec: &mut dyn FnMut(&Cmd) -> io::Result<Captured>,
) -> StepResult {
    output::header(tool.name, &tool.cmd);
    let (captured, secs) = timed(|| exec(&tool.cmd));
    let (failed, warnings) = match captured {
        Ok(captured) => {
            let findings = (tool.parse)(&captured.output, ctx, &tool.cmd);
            for finding in &findings {
                output::violation(&finding.violation);
            }
            let has_error = findings.iter().any(|f| f.is_error);
            let warnings = findings.iter().filter(|f| !f.is_error).count();
            let failed = !captured.success;
            if failed && !has_error {
                output::failure(
                    &format!(
                        "`{}` failed without a file:line; its output is above",
                        tool.cmd
                    ),
                    &(tool.fix)(&tool.cmd, Some(&captured.output)),
                );
            }
            (failed, warnings)
        }
        Err(error) => {
            let message = format!("could not run `{}`: {error}", tool.cmd.program.name());
            output::failure(&message, &(tool.fix)(&tool.cmd, None));
            (true, 0)
        }
    };
    StepResult {
        name: tool.name.into(),
        status: StepStatus::Ran {
            failed,
            warnings,
            secs,
        },
    }
}

/// Runs `work` and returns its value with the seconds it took. The only clock read in xtask.
fn timed<T>(work: impl FnOnce() -> T) -> (T, f64) {
    let started = Instant::now();
    let value = work();
    (value, started.elapsed().as_secs_f64())
}

fn reproduce(cmd: &Cmd, _output: Option<&str>) -> String {
    format!("run `{cmd}` to reproduce")
}

fn fmt_step() -> ToolStep {
    ToolStep {
        name: "fmt",
        cmd: Cmd::new(Program::Cargo, ["fmt", "--all", "--", "--check"]),
        parse: parse::fmt_findings,
        fix: reproduce,
    }
}

fn clippy_step(selectors: &[String], ci: bool) -> ToolStep {
    let mut args = vec!["clippy".to_string()];
    args.extend(selectors.iter().cloned());
    args.extend(["--all-targets", "--message-format=short"].map(String::from));
    if ci {
        args.extend(["--", "-D", "warnings"].map(String::from));
    }
    ToolStep {
        name: "clippy",
        cmd: Cmd::new(Program::Cargo, args),
        parse: parse::clippy_findings,
        fix: reproduce,
    }
}

fn nextest_step(selectors: &[String], ci: bool) -> ToolStep {
    let mut args = vec!["nextest".to_string(), "run".to_string()];
    args.extend(selectors.iter().cloned());
    args.push("--no-tests=pass".into());
    if ci {
        args.extend(["--profile", "ci"].map(String::from));
    }
    ToolStep {
        name: "nextest",
        cmd: Cmd::new(Program::Cargo, args),
        parse: parse_none,
        fix: nextest_fix,
    }
}

fn parse_none(_output: &str, _ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    Vec::new()
}

fn nextest_fix(cmd: &Cmd, output: Option<&str>) -> String {
    match output {
        None => "run `cargo xtask setup`".into(),
        Some(text) if text.contains("no such command: `nextest`") => {
            "run `cargo xtask setup`".into()
        }
        Some(_) => reproduce(cmd, output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Member;
    use std::path::PathBuf;

    fn context(members: Vec<Member>) -> Context {
        Context {
            root: PathBuf::from("/ws"),
            members,
        }
    }

    fn member(name: &str, dir: &str) -> Member {
        Member {
            name: name.into(),
            dir: dir.into(),
            path_deps: Vec::new(),
        }
    }

    fn ok_output(cmd: &Cmd) -> Captured {
        Captured {
            success: true,
            output: cmd.to_string(),
        }
    }

    fn no_findings(_output: &str, _ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
        Vec::new()
    }

    fn fake_tool(name: &'static str, program: &'static str, args: &[&str]) -> Step {
        Step::Tool(ToolStep {
            name,
            cmd: Cmd::new(Program::Named(program), args),
            parse: no_findings,
            fix: reproduce,
        })
    }

    fn one_violation(_ctx: &Context) -> Vec<Violation> {
        vec![Violation {
            path: "docs/a.md".into(),
            line: 2,
            message: "bad link".into(),
            fix: "fix the link".into(),
        }]
    }

    static FAILING_CHECK: StaticCheck = StaticCheck {
        name: "fake-static",
        run: one_violation,
    };

    fn tool_commands(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .filter_map(|step| match step {
                Step::Tool(tool) => Some(tool.cmd.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_failing_step_does_not_stop_the_later_ones() {
        let steps = vec![
            fake_tool("first", "fake", &["fail"]),
            Step::Static(&FAILING_CHECK),
            fake_tool("last", "fake", &["pass"]),
        ];
        let mut calls = Vec::new();
        let mut exec = |cmd: &Cmd| {
            calls.push(cmd.to_string());
            Ok(Captured {
                success: !cmd.args.iter().any(|arg| arg == "fail"),
                output: "boom\n".into(),
            })
        };
        let results = run_all(steps, &context(Vec::new()), &mut exec);
        assert_eq!(results.len(), 3);
        assert!(matches!(
            results[0].status,
            StepStatus::Ran { failed: true, .. }
        ));
        assert!(matches!(
            results[1].status,
            StepStatus::Ran { failed: true, .. }
        ));
        assert!(matches!(
            results[2].status,
            StepStatus::Ran { failed: false, .. }
        ));
        assert_eq!(
            calls,
            vec!["fake fail".to_string(), "fake pass".to_string()]
        );
    }

    #[test]
    fn a_program_that_cannot_start_fails_its_step_and_the_next_step_runs() {
        let steps = vec![
            fake_tool("missing", "missing", &["x"]),
            fake_tool("present", "fake", &["x"]),
        ];
        let mut exec = |cmd: &Cmd| {
            if cmd.program.name() == "missing" {
                Err(io::Error::new(io::ErrorKind::NotFound, "no such file"))
            } else {
                Ok(ok_output(cmd))
            }
        };
        let results = run_all(steps, &context(Vec::new()), &mut exec);
        assert_eq!(results.len(), 2);
        assert!(matches!(
            results[0].status,
            StepStatus::Ran { failed: true, .. }
        ));
        assert!(matches!(
            results[1].status,
            StepStatus::Ran { failed: false, .. }
        ));
    }

    #[test]
    fn an_empty_package_list_is_nothing_and_skips_clippy_and_nextest() {
        assert_eq!(Scope::packages(vec![]), Scope::Nothing);
        let steps = plan(&Scope::Nothing, false);
        assert_eq!(steps.len(), 3);
        assert!(matches!(&steps[0], Step::Tool(tool) if tool.name == "fmt"));
        assert!(matches!(
            steps[1],
            Step::Skipped {
                name: "clippy",
                reason: "no changed packages"
            }
        ));
        assert!(matches!(
            steps[2],
            Step::Skipped {
                name: "nextest",
                reason: "no changed packages"
            }
        ));
    }

    #[test]
    fn plan_commands_for_the_workspace_in_both_modes() {
        assert_eq!(
            tool_commands(&plan(&Scope::Workspace, false)),
            [
                "cargo fmt --all -- --check",
                "cargo clippy --workspace --all-targets --message-format=short",
                "cargo nextest run --workspace --no-tests=pass",
            ]
        );
        assert_eq!(
            tool_commands(&plan(&Scope::Workspace, true)),
            [
                "cargo fmt --all -- --check",
                "cargo clippy --workspace --all-targets --message-format=short -- -D warnings",
                "cargo nextest run --workspace --no-tests=pass --profile ci",
            ]
        );
    }

    #[test]
    fn clippy_command_for_named_packages() {
        let scope = Scope::packages(vec!["a".into(), "b".into()]);
        assert_eq!(
            tool_commands(&plan(&scope, false))[1],
            "cargo clippy -p a -p b --all-targets --message-format=short"
        );
    }

    #[test]
    fn clippy_repro_narrows_to_the_owning_package_in_ci() {
        let members = vec![member("xtask", "xtask")];
        let steps = plan(&Scope::Workspace, true);
        let Step::Tool(clippy) = &steps[1] else {
            panic!("clippy is a tool step");
        };
        let output = "xtask/src/main.rs:4:9: error: unused variable: `unused`\n";
        let findings = (clippy.parse)(output, &context(members), &clippy.cmd);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].is_error);
        assert_eq!(
            findings[0].violation.fix,
            "`cargo clippy -p xtask --all-targets -- -D warnings` shows the full diagnostic and clippy's suggested change"
        );
    }

    #[test]
    fn clippy_repro_narrows_to_the_owning_package_without_ci() {
        let members = vec![member("a", "crates/a"), member("b", "crates/b")];
        let steps = plan(&Scope::packages(vec!["a".into(), "b".into()]), false);
        let Step::Tool(clippy) = &steps[1] else {
            panic!("clippy is a tool step");
        };
        let output = "crates/a/src/lib.rs:1:1: warning: unused import\n";
        let findings = (clippy.parse)(output, &context(members), &clippy.cmd);
        assert_eq!(findings.len(), 1);
        assert!(!findings[0].is_error);
        assert_eq!(
            findings[0].violation.fix,
            "`cargo clippy -p a --all-targets` shows the full diagnostic and clippy's suggested change"
        );
    }

    #[test]
    fn clippy_keeps_the_selectors_when_no_package_owns_the_file() {
        let steps = plan(&Scope::packages(vec!["a".into(), "b".into()]), false);
        let Step::Tool(clippy) = &steps[1] else {
            panic!("clippy is a tool step");
        };
        let output = "docs/x.md:3:1: warning: stray text\n";
        let findings = (clippy.parse)(output, &context(Vec::new()), &clippy.cmd);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].violation.fix,
            "`cargo clippy -p a -p b --all-targets` shows the full diagnostic and clippy's suggested change"
        );
    }

    #[test]
    fn nextest_points_at_setup_when_the_subcommand_is_missing() {
        let cmd = Cmd::new(Program::Cargo, ["nextest", "run"]);
        assert_eq!(
            nextest_fix(&cmd, Some("error: no such command: `nextest`\n")),
            "run `cargo xtask setup`"
        );
        assert_eq!(nextest_fix(&cmd, None), "run `cargo xtask setup`");
        assert_eq!(
            nextest_fix(&cmd, Some("1 test failed")),
            "run `cargo nextest run` to reproduce"
        );
    }

    #[test]
    fn a_clean_run_reports_no_failure() {
        let steps = vec![fake_tool("quiet", "fake", &["x"])];
        let mut exec = |cmd: &Cmd| Ok(ok_output(cmd));
        let results = run_all(steps, &context(Vec::new()), &mut exec);
        assert!(matches!(
            results[0].status,
            StepStatus::Ran {
                failed: false,
                warnings: 0,
                ..
            }
        ));
    }
}

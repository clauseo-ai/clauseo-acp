//! The steps `cargo xtask check` runs. `plan` lists them in order; each builder says what one tool
//! runs and how its output is read. Adding an external step is one `ToolStep` here.

use crate::checks;
use crate::parse;
use crate::process::{Cmd, Program};
use crate::steps::{Scope, Step, ToolStep};
use crate::violation::Finding;
use crate::workspace::Context;

const NO_PACKAGES: &str = "no changed packages";

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

/// nextest's output has no `file:line` to parse, so it yields no findings.
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
}

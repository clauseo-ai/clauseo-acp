//! The steps `cargo xtask check` runs. `plan` lists them in order; each builder says what one tool
//! runs and how its output is read. Adding an external step is one `ToolStep` here, or in
//! `policy.rs` for a repo-policy tool.

use super::policy;
use crate::checks;
use crate::parse;
use crate::process::{Cmd, Program};
use crate::steps::{Scope, Step, ToolStep};
use crate::violation::Finding;
use crate::workspace::Context;

const NO_PACKAGES: &str = "no changed packages";

/// The steps for `scope`, in run order: fmt, clippy, deny, shear, typos, taplo, gitleaks, every
/// static check, nextest. With `Scope::Nothing`, clippy and nextest are skipped; the others never
/// depend on the scope. `ci` picks the CI variants of the clippy and nextest commands.
/// `gitleaks_base` is the commit this branch forked from; gitleaks scans the commits after it, or
/// all history when it is `None`.
pub(crate) fn plan(scope: &Scope, ci: bool, gitleaks_base: Option<&str>) -> Vec<Step> {
    let selectors = scope.selectors();
    let mut steps = vec![Step::Tool(fmt_step())];
    steps.push(match &selectors {
        Some(selectors) => Step::Tool(clippy_step(selectors, ci)),
        None => skipped("clippy"),
    });
    steps.extend(
        [
            policy::deny_step(),
            policy::shear_step(),
            policy::typos_step(),
            policy::taplo_step(),
            policy::gitleaks_step(gitleaks_base),
        ]
        .map(Step::Tool),
    );
    steps.extend(checks::ALL.iter().map(Step::Static));
    steps.push(match &selectors {
        Some(selectors) => Step::Tool(nextest_step(selectors, ci)),
        None => skipped("nextest"),
    });
    steps
}

fn skipped(name: &'static str) -> Step {
    Step::Skipped {
        name,
        reason: NO_PACKAGES,
    }
}

pub(super) fn reproduce(cmd: &Cmd, _output: Option<&str>) -> String {
    format!("run `{cmd}` to reproduce")
}

fn fmt_step() -> ToolStep {
    ToolStep {
        name: "fmt",
        cmd: Cmd::new(Program::Cargo, ["fmt", "--all", "--", "--check"]),
        parse: parse::fmt_findings,
        fix: reproduce,
        render: None,
    }
}

fn clippy_step(selectors: &[String], ci: bool) -> ToolStep {
    let mut args = vec!["clippy".to_owned()];
    args.extend(selectors.iter().cloned());
    args.extend(["--all-targets", "--message-format=json-diagnostic-short"].map(String::from));
    if ci {
        args.extend(["--", "-D", "warnings"].map(String::from));
    }
    ToolStep {
        name: "clippy",
        cmd: Cmd::new(Program::Cargo, args),
        parse: parse::clippy_findings,
        fix: reproduce,
        render: Some(parse::clippy_render),
    }
}

fn nextest_step(selectors: &[String], ci: bool) -> ToolStep {
    let mut args = vec!["nextest".to_owned(), "run".to_owned()];
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
        render: None,
    }
}

/// For output with no `file:line` to parse (nextest, cargo-deny, cargo-shear): no findings.
pub(super) fn parse_none(_output: &str, _ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    Vec::new()
}

fn nextest_fix(cmd: &Cmd, output: Option<&str>) -> String {
    match output {
        None => "run `cargo xtask setup`".into(),
        Some(text) if text.contains("no such command") => "run `cargo xtask setup`".into(),
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

    /// Each step's name, with `skipped` after the name of a skipped step.
    fn step_names(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .map(|step| match step {
                Step::Tool(tool) => tool.name.to_owned(),
                Step::Static(check) => check.name.to_owned(),
                Step::Skipped { name, reason } => {
                    assert_eq!(*reason, "no changed packages");
                    format!("{name} skipped")
                }
            })
            .collect()
    }

    /// The names a plan should have: the tool steps around every registered static check.
    fn expected_names(clippy: &str, nextest: &str) -> Vec<String> {
        let mut names: Vec<String> = ["fmt", clippy, "deny", "shear", "typos", "taplo", "gitleaks"]
            .map(str::to_owned)
            .to_vec();
        names.extend(checks::ALL.iter().map(|check| check.name.to_owned()));
        names.push(nextest.to_owned());
        names
    }

    #[test]
    fn an_empty_package_list_is_nothing_and_skips_only_clippy_and_nextest() {
        assert_eq!(Scope::packages(vec![]), Scope::Nothing);
        assert_eq!(
            step_names(&plan(&Scope::Nothing, false, None)),
            expected_names("clippy skipped", "nextest skipped")
        );
    }

    #[test]
    fn plan_runs_every_step_in_order_for_the_workspace() {
        assert_eq!(
            step_names(&plan(&Scope::Workspace, false, None)),
            expected_names("clippy", "nextest")
        );
    }

    #[test]
    fn plan_commands_for_the_workspace_in_both_modes() {
        assert_eq!(
            tool_commands(&plan(&Scope::Workspace, false, Some("abc"))),
            [
                "cargo fmt --all -- --check",
                "cargo clippy --workspace --all-targets --message-format=json-diagnostic-short",
                "cargo deny --locked check",
                "cargo shear --deny-warnings",
                "typos --format brief",
                "taplo fmt --check",
                "gitleaks git --redact --no-banner --verbose --no-color --log-opts=abc..HEAD",
                "cargo nextest run --workspace --no-tests=pass",
            ]
        );
        let ci = tool_commands(&plan(&Scope::Workspace, true, Some("abc")));
        assert_eq!(
            ci[1],
            "cargo clippy --workspace --all-targets --message-format=json-diagnostic-short -- -D warnings"
        );
        assert_eq!(
            ci[7],
            "cargo nextest run --workspace --no-tests=pass --profile ci"
        );
    }

    #[test]
    fn clippy_command_for_named_packages() {
        let scope = Scope::packages(vec!["a".into(), "b".into()]);
        assert_eq!(
            tool_commands(&plan(&scope, false, None))[1],
            "cargo clippy -p a -p b --all-targets --message-format=json-diagnostic-short"
        );
    }

    #[test]
    fn clippy_repro_narrows_to_the_owning_package_in_ci() {
        let members = vec![member("xtask", "xtask")];
        let steps = plan(&Scope::Workspace, true, None);
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
        let steps = plan(&Scope::packages(vec!["a".into(), "b".into()]), false, None);
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
        let steps = plan(&Scope::packages(vec!["a".into(), "b".into()]), false, None);
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

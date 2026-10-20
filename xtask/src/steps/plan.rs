//! The steps `cargo xtask check` runs. `plan` lists them in order; each builder says what one tool
//! runs and how its output is read. Adding an external step is one `ToolStep` here, or in
//! `policy.rs` for a repo-policy tool.

use super::policy;
use super::ui;
use crate::checks;
use crate::parse;
use crate::process::{Cmd, Program};
use crate::steps::{Scope, Step, ToolStep};
use crate::violation::Finding;
use crate::workspace::Context;

const NO_PACKAGES: &str = "no changed packages";

/// The steps for `scope`, in run order: fmt, clippy, deny, shear, typos, taplo, gitleaks, every
/// static check, the six ui steps, nextest. With `Scope::Nothing`, clippy and nextest are skipped;
/// the others never depend on the scope. `ci` picks the CI variants of the clippy and nextest
/// commands. `gitleaks_range` is the git range gitleaks scans (`<base>..HEAD`), or `None` for all
/// history. `pnpm_found` says whether pnpm starts: without it, the ui steps are skipped unless
/// `ci`.
pub(crate) fn plan(
    scope: &Scope,
    ci: bool,
    gitleaks_range: Option<&str>,
    pnpm_found: bool,
) -> Vec<Step> {
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
            policy::gitleaks_step(gitleaks_range),
        ]
        .map(Step::Tool),
    );
    steps.extend(checks::ALL.iter().map(Step::Static));
    steps.extend(ui_steps(pnpm_found, ci));
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

/// The ui steps. Without pnpm they are skipped locally; in CI they run, and fail there.
fn ui_steps(pnpm_found: bool, ci: bool) -> Vec<Step> {
    ui::steps()
        .into_iter()
        .map(|tool| {
            if pnpm_found || ci {
                Step::Tool(tool)
            } else {
                Step::Skipped {
                    name: tool.name,
                    reason: ui::NO_PNPM,
                }
            }
        })
        .collect()
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
        fix: setup_or_reproduce,
        render: None,
    }
}

/// For output with no `file:line` to parse (nextest, cargo-deny, cargo-shear): no findings.
pub(super) fn parse_none(_output: &str, _ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    Vec::new()
}

/// Points at `cargo xtask setup` when the tool is missing: it could not start (`None`), or cargo
/// has no such subcommand. Otherwise `otherwise`.
pub(super) fn setup_or(output: Option<&str>, otherwise: String) -> String {
    match output {
        Some(text) if !text.contains("no such command") => otherwise,
        _ => "run `cargo xtask setup`".into(),
    }
}

/// The fix for a pinned tool with no `file:line` in its output: setup when it is missing, else the
/// command that reproduces the failure.
pub(super) fn setup_or_reproduce(cmd: &Cmd, output: Option<&str>) -> String {
    setup_or(output, reproduce(cmd, output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Member;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn context(members: Vec<Member>) -> Context {
        Context {
            root: PathBuf::from("/ws"),
            members,
            dependencies: BTreeMap::new(),
            load_resolved_graph: false,
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

    /// The command of each tool step named in `names`, in plan order.
    fn commands_named(steps: &[Step], names: &[&str]) -> Vec<String> {
        steps
            .iter()
            .filter_map(|step| match step {
                Step::Tool(tool) if names.contains(&tool.name) => Some(tool.cmd.to_string()),
                _ => None,
            })
            .collect()
    }

    /// The tool step named `name`. A plan without one fails the test here.
    fn tool<'a>(steps: &'a [Step], name: &str) -> &'a ToolStep {
        let found = steps.iter().find_map(|step| match step {
            Step::Tool(found) if found.name == name => Some(found),
            _ => None,
        });
        let Some(found) = found else {
            panic!("the plan has a `{name}` tool step");
        };
        found
    }

    /// Each step's name, with `skipped` after the name of a skipped step.
    fn step_names(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .map(|step| match step {
                Step::Tool(tool) => tool.name.to_owned(),
                Step::Static(check) => check.name.to_owned(),
                Step::Skipped { name, .. } => format!("{name} skipped"),
            })
            .collect()
    }

    /// The reason of each skipped step, by name, in plan order.
    fn skip_reasons(steps: &[Step]) -> Vec<(&'static str, &'static str)> {
        steps
            .iter()
            .filter_map(|step| match step {
                Step::Skipped { name, reason } => Some((*name, *reason)),
                _ => None,
            })
            .collect()
    }

    /// The ui step names, in run order, when they run.
    const UI: [&str; 6] = [
        "pnpm-install",
        "tsc",
        "oxlint",
        "oxfmt",
        "vitest",
        "vite-build",
    ];

    /// The ui step names, in run order, when they are skipped.
    const UI_SKIPPED: [&str; 6] = [
        "pnpm-install skipped",
        "tsc skipped",
        "oxlint skipped",
        "oxfmt skipped",
        "vitest skipped",
        "vite-build skipped",
    ];

    /// The names a plan should have: the tool steps around every registered static check, then
    /// the ui steps (`ui`: running or skipped), then nextest.
    fn expected_names(clippy: &str, ui: [&str; 6], nextest: &str) -> Vec<String> {
        let mut names: Vec<String> = ["fmt", clippy, "deny", "shear", "typos", "taplo", "gitleaks"]
            .map(str::to_owned)
            .to_vec();
        names.extend(checks::ALL.iter().map(|check| check.name.to_owned()));
        names.extend(ui.map(str::to_owned));
        names.push(nextest.to_owned());
        names
    }

    #[test]
    fn an_empty_package_list_is_nothing_and_skips_only_clippy_and_nextest() {
        assert_eq!(Scope::packages(vec![]), Scope::Nothing);
        let steps = plan(&Scope::Nothing, false, None, true);
        assert_eq!(
            step_names(&steps),
            expected_names("clippy skipped", UI, "nextest skipped")
        );
        assert_eq!(
            skip_reasons(&steps),
            [("clippy", NO_PACKAGES), ("nextest", NO_PACKAGES)]
        );
    }

    #[test]
    fn plan_runs_every_step_in_order_for_the_workspace() {
        assert_eq!(
            step_names(&plan(&Scope::Workspace, false, None, true)),
            expected_names("clippy", UI, "nextest")
        );
    }

    #[test]
    fn plan_commands_for_the_workspace_in_both_modes() {
        assert_eq!(
            tool_commands(&plan(&Scope::Workspace, false, Some("abc..HEAD"), true)),
            [
                "cargo fmt --all -- --check",
                "cargo clippy --workspace --all-targets --message-format=json-diagnostic-short",
                "cargo deny --locked check",
                "cargo shear --deny-warnings",
                "typos --format brief",
                "taplo fmt --check",
                "gitleaks git --redact --no-banner --verbose --no-color --log-opts=abc..HEAD",
                "pnpm -C ui install --frozen-lockfile",
                "pnpm -C ui/app run typecheck --pretty false",
                "pnpm -C ui/app run lint --format unix",
                "pnpm -C ui run fmt:check",
                "pnpm -C ui/app run test",
                "pnpm -C ui/app run build",
                "cargo nextest run --workspace --no-tests=pass",
            ]
        );
        let ci = plan(&Scope::Workspace, true, Some("abc..HEAD"), true);
        assert_eq!(
            tool(&ci, "clippy").cmd.to_string(),
            "cargo clippy --workspace --all-targets --message-format=json-diagnostic-short -- -D warnings"
        );
        assert_eq!(
            tool(&ci, "nextest").cmd.to_string(),
            "cargo nextest run --workspace --no-tests=pass --profile ci"
        );
    }

    #[test]
    fn the_ui_steps_run_in_order_after_the_static_checks_and_before_nextest() {
        let steps = plan(&Scope::Workspace, false, None, true);
        let names = step_names(&steps);
        let first = names.iter().position(|name| name == "pnpm-install");
        let last = names.iter().position(|name| name == "vite-build");
        let (Some(first), Some(last)) = (first, last) else {
            panic!("the ui steps are in the plan: {names:?}");
        };
        assert_eq!(names[first..=last], UI);
        assert_eq!(names[first - 1], checks::ALL[checks::ALL.len() - 1].name);
        assert_eq!(names[last + 1], "nextest");
    }

    #[test]
    fn without_pnpm_locally_the_ui_steps_are_skipped_with_the_note() {
        let steps = plan(&Scope::Workspace, false, None, false);
        assert_eq!(
            step_names(&steps),
            expected_names("clippy", UI_SKIPPED, "nextest")
        );
        let ui_reasons: Vec<(&str, &str)> = skip_reasons(&steps)
            .into_iter()
            .filter(|(name, _)| UI.contains(name))
            .collect();
        assert_eq!(ui_reasons, UI.map(|name| (name, ui::NO_PNPM)));
        assert!(
            tool_commands(&steps)
                .iter()
                .all(|cmd| !cmd.starts_with("pnpm"))
        );
    }

    #[test]
    fn in_ci_the_ui_steps_run_even_without_pnpm() {
        let steps = plan(&Scope::Workspace, true, None, false);
        assert_eq!(step_names(&steps), expected_names("clippy", UI, "nextest"));
        assert_eq!(
            commands_named(&steps, &UI),
            [
                "pnpm -C ui install --frozen-lockfile",
                "pnpm -C ui/app run typecheck --pretty false",
                "pnpm -C ui/app run lint --format unix",
                "pnpm -C ui run fmt:check",
                "pnpm -C ui/app run test",
                "pnpm -C ui/app run build",
            ]
        );
    }

    #[test]
    fn clippy_command_for_named_packages() {
        let scope = Scope::packages(vec!["a".into(), "b".into()]);
        assert_eq!(
            tool(&plan(&scope, false, None, true), "clippy")
                .cmd
                .to_string(),
            "cargo clippy -p a -p b --all-targets --message-format=json-diagnostic-short"
        );
    }

    #[test]
    fn clippy_repro_narrows_to_the_owning_package_in_ci() {
        let members = vec![member("xtask", "xtask")];
        let steps = plan(&Scope::Workspace, true, None, true);
        let clippy = tool(&steps, "clippy");
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
        let steps = plan(
            &Scope::packages(vec!["a".into(), "b".into()]),
            false,
            None,
            true,
        );
        let clippy = tool(&steps, "clippy");
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
        let steps = plan(
            &Scope::packages(vec!["a".into(), "b".into()]),
            false,
            None,
            true,
        );
        let clippy = tool(&steps, "clippy");
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
            setup_or_reproduce(&cmd, Some("error: no such command: `nextest`\n")),
            "run `cargo xtask setup`"
        );
        assert_eq!(setup_or_reproduce(&cmd, None), "run `cargo xtask setup`");
        assert_eq!(
            setup_or_reproduce(&cmd, Some("1 test failed")),
            "run `cargo nextest run` to reproduce"
        );
    }
}

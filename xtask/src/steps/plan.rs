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
use std::time::Duration;

const NO_PACKAGES: &str = "no changed packages";

/// How long `cargo fmt --check` may run: it formats the workspace in seconds.
const FMT_TIMEOUT: Duration = Duration::from_mins(5);
/// How long clippy and nextest may run: each can build the whole workspace, Tauri included, cold
/// on a slow machine, and nextest then runs every test.
const BUILD_TIMEOUT: Duration = Duration::from_mins(45);

/// The steps for `scope`, in run order: fmt, clippy, clippy-featureless, deny, shear, typos, taplo,
/// gitleaks, every static check, the six ui steps, nextest, nextest-specta, doctest. With
/// `Scope::Nothing`, the five package steps (clippy, clippy-featureless, nextest, nextest-specta,
/// doctest) are skipped; the others never depend on the scope. `ci` picks the CI variants of the
/// clippy and nextest commands.
/// `gitleaks_range` is the git range gitleaks scans (`<base>..HEAD`), or `None` for all history.
/// `pnpm_found` says whether pnpm starts: without it, the ui steps are skipped unless `ci`.
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
    steps.push(protocol_step(scope, FEATURELESS_CLIPPY_NAME, || {
        featureless_clippy_step(ci)
    }));
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
    steps.push(protocol_step(scope, SPECTA_NAME, || specta_step(ci)));
    steps.push(protocol_step(scope, DOCTEST_NAME, doctest_step));
    steps
}

fn skipped(name: &'static str) -> Step {
    Step::Skipped {
        name,
        reason: NO_PACKAGES,
    }
}

/// The package whose `specta` export, featureless build and doc tests the protocol steps check.
/// Protocol is the one feature-gated crate, which is why its name is here and not read from the
/// manifests.
const PROTOCOL: &str = "clauseo-protocol";
const FEATURELESS_CLIPPY_NAME: &str = "clippy-featureless";
const SPECTA_NAME: &str = "nextest-specta";
const DOCTEST_NAME: &str = "doctest";
const NOT_CHANGED: &str = "clauseo-protocol is not a changed package";

/// The test targets behind `specta`: protocol's TypeScript export and `enum_table`. Cargo selects
/// them with `--test`, so only these two targets build. A nextest `-E` filter applies after the
/// build, which would build all eleven of protocol's test targets to run two. A renamed target
/// makes cargo fail the step and name the similar one.
const SPECTA_TESTS: [&str; 2] = ["specta_export", "enum_table"];

/// A step for protocol only: it runs on the workspace or when protocol is a named package, and is
/// skipped otherwise, so quick mode leaves protocol's steps out when protocol did not change.
fn protocol_step(scope: &Scope, name: &'static str, tool: impl FnOnce() -> ToolStep) -> Step {
    match scope {
        Scope::Workspace => Step::Tool(tool()),
        Scope::Packages(names) if names.iter().any(|named| named == PROTOCOL) => Step::Tool(tool()),
        Scope::Packages(_) => Step::Skipped {
            name,
            reason: NOT_CHANGED,
        },
        Scope::Nothing => skipped(name),
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
        cmd: Cmd::new(
            Program::Cargo,
            ["fmt", "--all", "--", "--check"],
            FMT_TIMEOUT,
        ),
        parse: parse::fmt_findings,
        fix: reproduce,
        render: None,
    }
}

/// The flags of the clippy step after its package selectors: every target and every feature, so
/// that tests, benches, code behind a feature and `required-features` targets are linted too.
pub(crate) const CLIPPY_FLAGS: [&str; 3] = [
    "--all-targets",
    "--all-features",
    "--message-format=json-diagnostic-short",
];

fn clippy_step(selectors: &[String], ci: bool) -> ToolStep {
    let mut args = vec!["clippy".to_owned()];
    args.extend(selectors.iter().cloned());
    args.extend(CLIPPY_FLAGS.map(String::from));
    deny_warnings_in_ci(&mut args, ci);
    ToolStep {
        name: "clippy",
        cmd: Cmd::new(Program::Cargo, args, BUILD_TIMEOUT),
        parse: parse::clippy_findings,
        fix: reproduce,
        render: Some(parse::clippy_render),
    }
}

/// The `-D warnings` tail of a clippy step. In CI a warning fails the step; a local run shows it
/// and passes, as the warnings of any other package do.
fn deny_warnings_in_ci(args: &mut Vec<String>, ci: bool) {
    if ci {
        args.extend(["--", "-D", "warnings"].map(String::from));
    }
}

/// The featureless run: the build the other crates use, so a `specta`-only item fails here.
/// Feature-gated tests run in `specta_step`.
fn nextest_step(selectors: &[String], ci: bool) -> ToolStep {
    let mut args = vec!["nextest".to_owned(), "run".to_owned()];
    args.extend(selectors.iter().cloned());
    args.push("--no-tests=pass".into());
    if ci {
        args.extend(["--profile", "ci"].map(String::from));
    }
    ToolStep {
        name: "nextest",
        cmd: Cmd::new(Program::Cargo, args, BUILD_TIMEOUT),
        parse: parse_none,
        fix: setup_or_reproduce,
        render: None,
    }
}

/// Clippy on protocol with no features: the build every other crate gets. In CI `-D warnings`,
/// as the main clippy step has, so a warning that only this build shows fails the check; the
/// all-features clippy step cannot see it. Locally it shows the warning and passes.
fn featureless_clippy_step(ci: bool) -> ToolStep {
    let mut args = [
        "clippy",
        "-p",
        PROTOCOL,
        "--all-targets",
        "--message-format=json-diagnostic-short",
    ]
    .map(String::from)
    .to_vec();
    deny_warnings_in_ci(&mut args, ci);
    ToolStep {
        name: FEATURELESS_CLIPPY_NAME,
        cmd: Cmd::new(Program::Cargo, args, BUILD_TIMEOUT),
        parse: parse::clippy_findings,
        fix: reproduce,
        render: Some(parse::clippy_render),
    }
}

/// The `specta` tests, with the feature on. No `--no-tests=pass`: the run fails if none runs.
fn specta_step(ci: bool) -> ToolStep {
    let mut args = ["nextest", "run", "-p", PROTOCOL, "--features", "specta"]
        .map(String::from)
        .to_vec();
    args.extend(
        SPECTA_TESTS
            .iter()
            .flat_map(|test| ["--test", *test])
            .map(String::from),
    );
    if ci {
        args.extend(["--profile", "ci"].map(String::from));
    }
    ToolStep {
        name: SPECTA_NAME,
        cmd: Cmd::new(Program::Cargo, args, BUILD_TIMEOUT),
        parse: parse_none,
        fix: setup_or_reproduce,
        render: None,
    }
}

/// Protocol's doc tests, every feature on. nextest runs no doc tests, and protocol's
/// `compile_fail` examples are what show that only protocol builds a `ValidatedCommand` or an
/// `UnknownKind`; without this step nothing runs them.
fn doctest_step() -> ToolStep {
    ToolStep {
        name: DOCTEST_NAME,
        cmd: Cmd::new(
            Program::Cargo,
            ["test", "-p", PROTOCOL, "--doc", "--all-features"],
            BUILD_TIMEOUT,
        ),
        parse: parse_none,
        fix: reproduce,
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
mod tests;

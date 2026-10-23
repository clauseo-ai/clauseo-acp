use super::*;
use crate::workspace::Member;
use std::path::PathBuf;

fn context(members: Vec<Member>) -> Context {
    Context {
        root: PathBuf::from("/ws"),
        members: Ok(members),
        load_resolved_graph: false,
    }
}

fn member(name: &str, dir: &str) -> Member {
    Member {
        name: name.into(),
        dir: dir.into(),
        path_deps: Vec::new(),
        targets: Vec::new(),
        dependencies: crate::workspace::PackageDependencies::default(),
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

/// The package steps' names, running: clippy, the featureless clippy, nextest, nextest-specta,
/// doctest.
const PACKAGE: [&str; 5] = [
    "clippy",
    "clippy-featureless",
    "nextest",
    "nextest-specta",
    "doctest",
];

/// The names a plan should have: fmt, the two clippy steps (`package`: running or skipped), the
/// tool steps around every registered static check, the ui steps (`ui`: running or skipped), then
/// the two nextest steps and the doc tests (`package`).
fn expected_names(package: [&str; 5], ui: [&str; 6]) -> Vec<String> {
    let [clippy, featureless, nextest, specta, doctest] = package;
    let mut names: Vec<String> = [
        "fmt",
        clippy,
        featureless,
        "deny",
        "shear",
        "typos",
        "taplo",
        "gitleaks",
    ]
    .map(str::to_owned)
    .to_vec();
    names.extend(checks::ALL.iter().map(|check| check.name.to_owned()));
    names.extend(ui.map(str::to_owned));
    names.push(nextest.to_owned());
    names.push(specta.to_owned());
    names.push(doctest.to_owned());
    names
}

#[test]
fn an_empty_package_list_is_nothing_and_skips_every_package_step() {
    assert_eq!(Scope::packages(vec![]), Scope::Nothing);
    let steps = plan(&Scope::Nothing, false, None, true);
    assert_eq!(
        step_names(&steps),
        expected_names(
            [
                "clippy skipped",
                "clippy-featureless skipped",
                "nextest skipped",
                "nextest-specta skipped",
                "doctest skipped",
            ],
            UI
        )
    );
    assert_eq!(
        skip_reasons(&steps),
        [
            ("clippy", NO_PACKAGES),
            ("clippy-featureless", NO_PACKAGES),
            ("nextest", NO_PACKAGES),
            ("nextest-specta", NO_PACKAGES),
            ("doctest", NO_PACKAGES),
        ]
    );
}

#[test]
fn plan_runs_every_step_in_order_for_the_workspace() {
    assert_eq!(
        step_names(&plan(&Scope::Workspace, false, None, true)),
        expected_names(PACKAGE, UI)
    );
}

#[test]
fn plan_commands_for_the_workspace_in_both_modes() {
    assert_eq!(
        tool_commands(&plan(&Scope::Workspace, false, Some("abc..HEAD"), true)),
        [
            "cargo fmt --all -- --check",
            "cargo clippy --workspace --all-targets --all-features --message-format=json-diagnostic-short",
            "cargo clippy -p clauseo-protocol --all-targets --message-format=json-diagnostic-short",
            "cargo deny --locked check",
            "cargo shear --deny-warnings",
            "typos --format brief",
            "taplo fmt --check",
            "gitleaks git --redact --no-banner --verbose --no-color --log-opts=abc..HEAD",
            "pnpm -C ui install --frozen-lockfile",
            "pnpm -C ui/app run typecheck --pretty false",
            "pnpm -C ui/app run lint:types --format unix",
            "pnpm -C ui run fmt:check",
            "pnpm -C ui/app run test",
            "pnpm -C ui/app run build",
            "cargo nextest run --workspace --no-tests=pass",
            "cargo nextest run -p clauseo-protocol --features specta --test specta_export --test enum_table",
            "cargo test -p clauseo-protocol --doc --all-features",
        ]
    );
    let ci = plan(&Scope::Workspace, true, Some("abc..HEAD"), true);
    assert_eq!(
        tool(&ci, "clippy-featureless").cmd.to_string(),
        "cargo clippy -p clauseo-protocol --all-targets --message-format=json-diagnostic-short -- -D warnings"
    );
    assert_eq!(
        tool(&ci, "clippy").cmd.to_string(),
        "cargo clippy --workspace --all-targets --all-features --message-format=json-diagnostic-short -- -D warnings"
    );
    assert_eq!(
        tool(&ci, "nextest").cmd.to_string(),
        "cargo nextest run --workspace --no-tests=pass --profile ci"
    );
    assert_eq!(
        tool(&ci, "nextest-specta").cmd.to_string(),
        "cargo nextest run -p clauseo-protocol --features specta --test specta_export --test enum_table --profile ci"
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
    assert_eq!(step_names(&steps), expected_names(PACKAGE, UI_SKIPPED));
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
    assert_eq!(step_names(&steps), expected_names(PACKAGE, UI));
    assert_eq!(
        commands_named(&steps, &UI),
        [
            "pnpm -C ui install --frozen-lockfile",
            "pnpm -C ui/app run typecheck --pretty false",
            "pnpm -C ui/app run lint:types --format unix",
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
        "cargo clippy -p a -p b --all-targets --all-features --message-format=json-diagnostic-short"
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
        findings[0].violation.fix.to_string(),
        "`cargo clippy -p xtask --all-targets --all-features -- -D warnings` shows the full diagnostic and clippy's suggested change"
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
        findings[0].violation.fix.to_string(),
        "`cargo clippy -p a --all-targets --all-features` shows the full diagnostic and clippy's suggested change"
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
        findings[0].violation.fix.to_string(),
        "`cargo clippy -p a -p b --all-targets --all-features` shows the full diagnostic and clippy's suggested change"
    );
}

#[test]
fn the_specta_run_follows_the_featureless_run_and_fails_when_no_test_runs() {
    let steps = plan(&Scope::Workspace, false, None, true);
    let names = step_names(&steps);
    let Some(at) = names.iter().position(|name| name == "nextest") else {
        panic!("the plan has a nextest step: {names:?}");
    };
    assert_eq!(names.get(at + 1).map(String::as_str), Some(SPECTA_NAME));
    assert!(
        !tool(&steps, SPECTA_NAME)
            .cmd
            .to_string()
            .contains("--no-tests")
    );
}

#[test]
fn the_specta_run_selects_its_two_targets_through_cargo_not_a_filter() {
    let steps = plan(&Scope::Workspace, false, None, true);
    let args = tool(&steps, SPECTA_NAME).cmd.to_string();
    assert_eq!(
        args,
        "cargo nextest run -p clauseo-protocol --features specta --test specta_export --test enum_table"
    );
    assert!(!args.contains(" -E "), "{args}");
}

#[test]
fn the_featureless_clippy_is_scoped_to_protocol() {
    let scoped = plan(&Scope::packages(vec![PROTOCOL.into()]), false, None, true);
    tool(&scoped, FEATURELESS_CLIPPY_NAME);
    let other = plan(&Scope::packages(vec!["xtask".into()]), false, None, true);
    let reasons: Vec<(&str, &str)> = skip_reasons(&other)
        .into_iter()
        .filter(|(name, _)| *name == FEATURELESS_CLIPPY_NAME)
        .collect();
    assert_eq!(reasons, [(FEATURELESS_CLIPPY_NAME, NOT_CHANGED)]);
}

#[test]
fn the_featureless_clippy_reproduce_line_drops_all_features() {
    let members = vec![member("clauseo-protocol", "crates/protocol")];
    let output = "crates/protocol/src/number.rs:4:5: error: unused import: `PhantomData`\n";
    let steps = plan(&Scope::Workspace, true, None, true);
    let clippy = tool(&steps, FEATURELESS_CLIPPY_NAME);
    let findings = (clippy.parse)(output, &context(members.clone()), &clippy.cmd);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].is_error);
    assert_eq!(
        findings[0].violation.fix.to_string(),
        "`cargo clippy -p clauseo-protocol --all-targets -- -D warnings` shows the full diagnostic and clippy's suggested change"
    );
    let steps = plan(&Scope::Workspace, false, None, true);
    let clippy = tool(&steps, FEATURELESS_CLIPPY_NAME);
    let findings = (clippy.parse)(output, &context(members), &clippy.cmd);
    assert_eq!(
        findings[0].violation.fix.to_string(),
        "`cargo clippy -p clauseo-protocol --all-targets` shows the full diagnostic and clippy's suggested change"
    );
}

#[test]
fn the_featureless_clippy_denies_warnings_only_in_ci_like_the_main_clippy() {
    const DENY: &str = " -- -D warnings";
    let ci = plan(&Scope::Workspace, true, None, true);
    assert!(
        tool(&ci, FEATURELESS_CLIPPY_NAME)
            .cmd
            .to_string()
            .ends_with(DENY)
    );
    assert!(tool(&ci, "clippy").cmd.to_string().ends_with(DENY));
    let local = plan(&Scope::Workspace, false, None, true);
    assert!(
        !tool(&local, FEATURELESS_CLIPPY_NAME)
            .cmd
            .to_string()
            .contains("-D")
    );
    assert!(!tool(&local, "clippy").cmd.to_string().contains("-D"));
}

#[test]
fn the_specta_run_is_scoped_to_protocol() {
    let scoped = plan(
        &Scope::packages(vec!["xtask".into(), PROTOCOL.into()]),
        false,
        None,
        true,
    );
    tool(&scoped, SPECTA_NAME);
    let other = plan(&Scope::packages(vec!["xtask".into()]), false, None, true);
    let reasons: Vec<(&str, &str)> = skip_reasons(&other)
        .into_iter()
        .filter(|(name, _)| *name == SPECTA_NAME)
        .collect();
    assert_eq!(reasons, [(SPECTA_NAME, NOT_CHANGED)]);
}

/// Protocol's doc tests (its `compile_fail` examples among them) run with every feature, after
/// the specta run, whenever the other protocol steps run, and are skipped exactly when they are.
#[test]
fn the_doc_tests_run_after_the_specta_run_and_are_scoped_to_protocol() {
    let steps = plan(&Scope::Workspace, true, None, true);
    let names = step_names(&steps);
    assert_eq!(names.last().map(String::as_str), Some(DOCTEST_NAME));
    let doctest = tool(&steps, DOCTEST_NAME);
    assert_eq!(
        doctest.cmd.to_string(),
        "cargo test -p clauseo-protocol --doc --all-features"
    );
    assert_eq!(
        (doctest.fix)(&doctest.cmd, None),
        "run `cargo test -p clauseo-protocol --doc --all-features` to reproduce"
    );
    let scoped = plan(
        &Scope::packages(vec!["xtask".into(), PROTOCOL.into()]),
        false,
        None,
        true,
    );
    tool(&scoped, DOCTEST_NAME);
    let other = plan(&Scope::packages(vec!["xtask".into()]), false, None, true);
    let reasons: Vec<(&str, &str)> = skip_reasons(&other)
        .into_iter()
        .filter(|(name, _)| [SPECTA_NAME, FEATURELESS_CLIPPY_NAME, DOCTEST_NAME].contains(name))
        .collect();
    assert_eq!(
        reasons,
        [
            (FEATURELESS_CLIPPY_NAME, NOT_CHANGED),
            (SPECTA_NAME, NOT_CHANGED),
            (DOCTEST_NAME, NOT_CHANGED),
        ]
    );
}

#[test]
fn nextest_points_at_setup_when_the_subcommand_is_missing() {
    let cmd = Cmd::new(
        Program::Cargo,
        ["nextest", "run"],
        crate::process::TEST_TIMEOUT,
    );
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

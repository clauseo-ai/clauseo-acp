//! `--no-tests` (`without_tests`): the test steps are skipped, and only they, whatever the scope,
//! the mode and `--skip-ui`.

use super::*;

/// The test step names, in run order, when they run.
const TESTS: [&str; 5] = [
    "vitest",
    "playwright-unit",
    "nextest",
    "nextest-specta",
    "doctest",
];

/// `--no-tests` skips exactly the five test steps, with `--no-tests` as the reason, in CI or not,
/// with `--skip-ui` or without it, and for the workspace or named packages (`--quick`); every
/// other step and command is as the same plan without it, so CI's `-D warnings` stays on clippy.
#[test]
fn no_tests_skips_exactly_the_test_steps_with_quick_and_skip_ui() {
    let protocol = Scope::packages(vec![PROTOCOL.to_owned()]);
    for scope in [Scope::Workspace, protocol] {
        for ci in [false, true] {
            for skip_ui in [false, true] {
                let with = plan(&scope, ci, None, true, skip_ui);
                let without = without_tests(plan(&scope, ci, None, true, skip_ui));
                let case = format!("scope={scope:?} ci={ci} skip_ui={skip_ui}");
                let expected: Vec<String> = step_names(&with)
                    .into_iter()
                    .map(|name| {
                        if TESTS.contains(&name.as_str()) {
                            format!("{name} skipped")
                        } else {
                            name
                        }
                    })
                    .collect();
                assert_eq!(step_names(&without), expected, "{case}");
                let reasons = skip_reasons(&without);
                let no_tests: Vec<&str> = reasons
                    .iter()
                    .filter(|(_, reason)| *reason == NO_TESTS)
                    .map(|(name, _)| *name)
                    .collect();
                if skip_ui {
                    assert_eq!(no_tests, TESTS[2..], "{case}");
                    assert!(reasons.contains(&("vitest", SKIP_UI)), "{case}");
                    assert!(reasons.contains(&("playwright-unit", SKIP_UI)), "{case}");
                } else {
                    assert_eq!(no_tests, TESTS, "{case}");
                }
                let not_tests: Vec<String> = with
                    .iter()
                    .filter_map(|step| match step {
                        Step::Tool(tool) if !TESTS.contains(&tool.name) => {
                            Some(tool.cmd.to_string())
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(tool_commands(&without), not_tests, "{case}");
                if ci {
                    assert!(
                        tool(&without, "clippy")
                            .cmd
                            .to_string()
                            .ends_with("-- -D warnings"),
                        "{case}"
                    );
                    assert!(
                        tool(&without, "clippy-featureless")
                            .cmd
                            .to_string()
                            .ends_with("-- -D warnings"),
                        "{case}"
                    );
                }
            }
        }
    }
}

/// The test steps `without_tests` names are the test steps `plan` builds: each is in a full plan.
#[test]
fn every_test_step_is_a_step_of_the_plan() {
    let names = step_names(&plan(&Scope::Workspace, true, None, true, false));
    assert_eq!(TEST_STEPS, TESTS);
    for test in TESTS {
        assert!(names.iter().any(|name| name == test), "{test} in {names:?}");
    }
}

/// A test step the scope already skips keeps the scope's reason under `--no-tests`.
#[test]
fn no_tests_leaves_a_step_the_scope_skips_to_the_scope() {
    let steps = without_tests(plan(&Scope::Nothing, true, None, true, false));
    let reasons = skip_reasons(&steps);
    for name in ["nextest", "nextest-specta", "doctest"] {
        assert!(
            reasons.contains(&(name, NO_PACKAGES)),
            "{name}: {reasons:?}"
        );
    }
    assert!(reasons.contains(&("vitest", NO_TESTS)));
    assert!(reasons.contains(&("playwright-unit", NO_TESTS)));
}

//! Tests of the planted-case harness, and the registry test: every check in `ALL` has planted
//! cases, a failing and a passing one among them, in the list of its own `tests/planted.rs`.

use super::*;
use crate::checks::{ALL, Inspection, Noun, violation};
use crate::fix::Fix;
use crate::scratch::Scratch;
use crate::workspace::Context;

fn clean(_: &Context) -> Inspection {
    Inspection::new(1, Vec::new())
}

fn empty(_: &Context) -> Inspection {
    Inspection::default()
}

fn one_violation(_: &Context) -> Inspection {
    Inspection::new(1, vec![violation("a.toml", 2, "bad", Fix::Fake("fix it"))])
}

fn looked_at_nothing(_: &Context) -> Inspection {
    Inspection::new(0, vec![violation("a.toml", 2, "bad", Fix::Fake("fix it"))])
}

fn stopped(_: &Context) -> Inspection {
    Inspection::stopped(violation("a.toml", 1, "gone", Fix::Fake("restore it")))
}

fn step_text(_: &Context) -> Inspection {
    Inspection::new(1, vec![violation("a.toml", 2, "bad", Fix::tool("run it"))])
}

/// The file of the fake check, and the one its own cases come from.
const FAKE_SOURCE: &str = "xtask/src/checks/fake.rs";
const FAKE_LIST: &str = "xtask/src/checks/fake/tests/planted.rs";

/// A check named `fake` whose `run` is `run`, with `planted` as its cases.
const fn fake(run: fn(&Context) -> Inspection, planted: &'static [Planted]) -> StaticCheck {
    StaticCheck {
        planted,
        ..StaticCheck::fake(
            "fake",
            Noun {
                one: "row",
                many: "rows",
            },
            || "in a.toml".to_owned(),
            FAKE_SOURCE,
            run,
        )
    }
}

/// An empty scratch repo; the fake checks read nothing from it.
fn nothing() -> Repo {
    Repo::new(Scratch::new("planted-harness"))
}

fn verdict(expect: Expect, inspects: usize, run: fn(&Context) -> Inspection) -> Result<(), String> {
    Planted {
        name: "case",
        file: FAKE_LIST,
        expect,
        inspects,
        repo: nothing,
    }
    .verdict(&fake(run, &[]))
}

/// The line `one_violation` prints, built as a case builds it.
fn bad() -> Vec<String> {
    vec![fails("a.toml:2", "bad", Fix::Fake("fix it"))]
}

/// `fails` and `failure` print as the runner prints a violation and a failure.
#[test]
fn the_expected_lines_print_as_the_runner_prints() {
    assert_eq!(
        fails("a.toml:2", "bad", Fix::Fake("fix it")),
        violation("a.toml", 2, "bad", Fix::Fake("fix it")).to_string()
    );
    assert_eq!(
        failure("git failed", Fix::Fake("fix git")),
        "git failed\n    fix: fix git"
    );
}

/// A check's fix is its own enum: a step's text fails the case even when it prints as expected.
#[test]
fn a_fix_with_a_step_text_fails_the_case() {
    assert_eq!(
        verdict(
            Expect::Fails(|| vec![fails("a.toml:2", "bad", Fix::tool("run it"))]),
            1,
            step_text
        ),
        Err(
            "the fix `run it` is a step's text; give it a variant in the check's Fix enum"
                .to_owned()
        )
    );
}

#[test]
fn fails_needs_the_exact_text() {
    assert_eq!(verdict(Expect::Fails(bad), 1, one_violation), Ok(()));
    assert_eq!(
        verdict(
            Expect::Fails(|| vec![fails("a.toml:3", "bad", Fix::Fake("fix it"))]),
            1,
            one_violation
        ),
        Err("expected these failures:\na.toml:3: bad\n    fix: fix it\nfound:\na.toml:2: bad\n    fix: fix it".to_owned())
    );
    assert_eq!(
        verdict(
            Expect::Fails(|| vec![fails("a.toml:2", "bad", Fix::Fake("fix that"))]),
            1,
            one_violation
        ),
        Err("expected these failures:\na.toml:2: bad\n    fix: fix that\nfound:\na.toml:2: bad\n    fix: fix it".to_owned()),
        "another fix fails the case"
    );
    assert_eq!(
        verdict(Expect::Fails(Vec::new), 1, clean),
        Err("a failing case lists at least one failure; a case that passes is Passes".to_owned())
    );
}

/// The failures are those the check judged: a stopped check lists only what stopped it, and a
/// check that looked at nothing gets the zero-inspected failure too.
#[test]
fn fails_holds_the_failures_judged_by_the_check() {
    assert_eq!(
        verdict(
            Expect::Fails(|| vec![fails("a.toml:1", "gone", Fix::Fake("restore it"))]),
            0,
            stopped
        ),
        Ok(())
    );
    assert_eq!(
        verdict(Expect::Fails(bad), 0, looked_at_nothing),
        Err("expected these failures:\na.toml:2: bad\n    fix: fix it\nfound:\na.toml:2: bad\n    fix: fix it\n0 rows inspected in a.toml\n    fix: fake looks for rows in a.toml; restore them, or, if they moved, update the consts xtask/src/checks/fake.rs builds that place from".to_owned())
    );
}

#[test]
fn passes_needs_no_violation_and_something_inspected() {
    assert_eq!(verdict(Expect::Passes, 1, clean), Ok(()));
    assert_eq!(
        verdict(Expect::Passes, 1, one_violation),
        Err("expected a pass, found:\na.toml:2: bad\n    fix: fix it".to_owned())
    );
    assert_eq!(
        verdict(Expect::Passes, 0, empty),
        Err("expected a pass, found:\n0 rows inspected in a.toml\n    fix: fake looks for rows in a.toml; restore them, or, if they moved, update the consts xtask/src/checks/fake.rs builds that place from".to_owned())
    );
}

#[test]
fn a_count_that_differs_fails_the_case_and_says_both_numbers() {
    assert_eq!(
        verdict(Expect::Passes, 1, clean),
        Ok(()),
        "a case that states the count it reads passes"
    );
    assert_eq!(
        verdict(Expect::Passes, 2, clean),
        Err("expected 2 inspected, found 1".to_owned())
    );
    assert_eq!(
        verdict(Expect::Passes, 0, clean),
        Err("expected 0 inspected, found 1".to_owned())
    );
    assert_eq!(
        verdict(Expect::Fails(bad), 2, one_violation),
        Err("expected 2 inspected, found 1".to_owned()),
        "a count that differs is reported before the failures"
    );
}

/// The check runs on the case's repo, through the check's own `run`: the repo's root is the
/// context's root, and `Repo::files` writes each file, parents first.
#[test]
fn the_harness_runs_the_check_on_the_case_s_repo() {
    fn counts_files(ctx: &Context) -> Inspection {
        let files = std::fs::read_dir(ctx.root.join("d/e")).map_or(0, Iterator::count);
        let text = std::fs::read_to_string(ctx.root.join("d/e/a")).unwrap_or_default();
        Inspection::new(if text == "x" { files } else { 0 }, Vec::new())
    }
    fn two_files() -> Repo {
        Repo::files("harness-two-files", &[("d/e/a", "x"), ("d/e/b", "")])
    }
    let case = Planted {
        name: "two_files",
        file: FAKE_LIST,
        expect: Expect::Passes,
        inspects: 2,
        repo: two_files,
    };
    assert_eq!(case.verdict(&fake(counts_files, &[])), Ok(()));
}

/// `scratch` writes each file's bytes, parents first, into a directory it keeps.
#[test]
fn scratch_writes_each_file_with_its_parents() {
    let dir = scratch(
        "harness-bytes",
        &[("a/b/c.bin", b"\xff\x00".as_slice()), ("d", b"")],
    );
    assert_eq!(
        std::fs::read(dir.join("a/b/c.bin")).ok(),
        Some(vec![0xff, 0x00])
    );
    assert_eq!(std::fs::read(dir.join("d")).ok(), Some(Vec::new()));
}

const PASSES: Planted = Planted {
    name: "passes",
    file: FAKE_LIST,
    expect: Expect::Passes,
    inspects: 1,
    repo: nothing,
};

const FAILS: Planted = Planted {
    name: "fails",
    file: FAKE_LIST,
    expect: Expect::Fails(bad),
    inspects: 1,
    repo: nothing,
};

/// A case from another check's list, as when `CHECK.planted` points at the wrong `PLANTED`.
const ELSEWHERE: Planted = Planted {
    name: "elsewhere",
    file: "xtask/src/checks/other/tests/planted.rs",
    ..PASSES
};

/// The registry names a check with no case, no failing case or no passing case, and one whose
/// list is not its own; it runs no case, so a case its check gets wrong is its own test's to fail.
#[test]
fn the_planted_registry_names_each_check_it_misses() {
    assert_eq!(
        registry_problems(&[fake(clean, &[])]),
        ["fake has no planted cases"]
    );
    assert_eq!(
        registry_problems(&[fake(clean, &[PASSES])]),
        ["fake has no planted case that fails"]
    );
    assert_eq!(
        registry_problems(&[fake(one_violation, &[FAILS])]),
        ["fake has no planted case that passes"]
    );
    assert_eq!(
        registry_problems(&[fake(clean, &[PASSES, FAILS])]),
        Vec::<String>::new(),
        "the registry runs no case: `fails` would fail its own test on this check"
    );
    assert_eq!(
        registry_problems(&[fake(clean, &[FAILS, ELSEWHERE])]),
        [
            "fake's planted list holds `elsewhere` from xtask/src/checks/other/tests/planted.rs; \
             point its CHECK.planted at tests::planted::PLANTED, the list in \
             xtask/src/checks/fake/tests/planted.rs"
        ]
    );
}

/// A Windows `file!()` uses `\`; it names the same list as the check's file with `/`.
#[test]
fn the_registry_matches_a_list_written_with_backslashes() {
    const WINDOWS: Planted = Planted {
        file: "xtask\\src\\checks\\fake\\tests\\planted.rs",
        ..PASSES
    };
    assert_eq!(
        registry_problems(&[fake(clean, &[FAILS, WINDOWS])]),
        Vec::<String>::new()
    );
}

/// The problems come sorted, whatever the order of the checks.
#[test]
fn the_registry_sorts_its_problems() {
    let mut empty = fake(clean, &[]);
    empty.name = "a-fake";
    assert_eq!(
        registry_problems(&[fake(clean, &[PASSES]), empty]),
        [
            "a-fake has no planted cases",
            "fake has no planted case that fails"
        ]
    );
}

/// The registry test: every check in `ALL` has planted cases, a failing one and a passing one
/// among them, all from its own `tests/planted.rs`. Each case runs in the test `cases!` makes.
#[test]
fn every_check_in_all_has_its_own_failing_and_passing_planted_cases() {
    let problems = registry_problems(ALL);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

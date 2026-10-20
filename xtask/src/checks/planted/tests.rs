//! Tests of the planted-case harness, and the registry test: every check in `ALL` fails each of
//! its planted violations and passes each of its clean repos.

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
            "xtask/src/checks/fake.rs",
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
        expect,
        inspects,
        repo: nothing,
    }
    .verdict(&fake(run, &[]))
}

/// A check's fix is its own enum: a step's text fails the case even when it prints as expected.
#[test]
fn a_fix_with_a_step_text_fails_the_case() {
    assert_eq!(
        verdict(
            Expect::FailsWith(&["a.toml:2: bad\n    fix: run it"]),
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
fn fails_with_needs_the_exact_text() {
    assert_eq!(
        verdict(
            Expect::FailsWith(&["a.toml:2: bad\n    fix: fix it"]),
            1,
            one_violation
        ),
        Ok(())
    );
    assert_eq!(
        verdict(
            Expect::FailsWith(&["a.toml:3: bad\n    fix: fix it"]),
            1,
            one_violation
        ),
        Err("expected these failures:\na.toml:3: bad\n    fix: fix it\nfound:\na.toml:2: bad\n    fix: fix it".to_owned())
    );
    assert_eq!(
        verdict(Expect::FailsWith(&[]), 1, clean),
        Err("FailsWith needs at least one failure; a case that passes is Passes".to_owned())
    );
}

/// The failures are those the check judged: a stopped check lists only what stopped it, and a
/// check that looked at nothing gets the zero-inspected failure too.
#[test]
fn fails_with_holds_the_failures_judged_by_the_check() {
    assert_eq!(
        verdict(
            Expect::FailsWith(&["a.toml:1: gone\n    fix: restore it"]),
            0,
            stopped
        ),
        Ok(())
    );
    assert_eq!(
        verdict(
            Expect::FailsWith(&["a.toml:2: bad\n    fix: fix it"]),
            0,
            looked_at_nothing
        ),
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
        verdict(
            Expect::FailsWith(&["a.toml:2: bad\n    fix: fix it"]),
            2,
            one_violation
        ),
        Err("expected 2 inspected, found 1".to_owned()),
        "a count that differs is reported before the failures"
    );
}

/// The check runs on the case's repo, through the check's own `run`: the repo's root is the
/// context's root.
#[test]
fn the_harness_runs_the_check_on_the_case_s_repo() {
    fn counts_files(ctx: &Context) -> Inspection {
        let files = std::fs::read_dir(&ctx.root).map_or(0, Iterator::count);
        Inspection::new(files, Vec::new())
    }
    fn two_files() -> Repo {
        let dir = Scratch::new("planted-two-files");
        Scratch::write(dir.join("a"), "").expect("write a");
        Scratch::write(dir.join("b"), "").expect("write b");
        Repo::new(dir)
    }
    let case = Planted {
        name: "two_files",
        expect: Expect::Passes,
        inspects: 2,
        repo: two_files,
    };
    assert_eq!(case.verdict(&fake(counts_files, &[])), Ok(()));
}

const PASSES: Planted = Planted {
    name: "passes",
    expect: Expect::Passes,
    inspects: 1,
    repo: nothing,
};

const FAILS: Planted = Planted {
    name: "fails",
    expect: Expect::FailsWith(&["a.toml:2: bad\n    fix: fix it"]),
    inspects: 1,
    repo: nothing,
};

/// The registry names a check with no case, no failing case or no passing case, and a case
/// whose check does not do what its expectation says.
#[test]
fn the_planted_registry_names_each_check_and_case_it_misses() {
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
        ["fake found nothing in its planted case `fails`, which holds a violation"]
    );
    assert_eq!(
        registry_problems(&[fake(one_violation, &[PASSES, FAILS])]),
        [
            "fake failed its planted case `passes`, which holds none:\na.toml:2: bad\n    fix: fix it"
        ]
    );
}

/// The registry test: every check in `ALL` has planted cases, a failing one and a passing one
/// among them, finds a violation or a failure in each failing case's repo and none in each
/// passing case's. The per-case tests pin the exact text; this one holds `ALL` to the rule.
#[test]
fn every_check_in_all_fails_its_planted_violations_and_passes_its_clean_repos() {
    let problems = registry_problems(ALL);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

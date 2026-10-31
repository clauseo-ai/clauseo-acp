//! Tests of the wire pass: a diff that changes `crates/protocol/src/wire.rs` runs a second pass on
//! that file alone with protocol's `specta` feature, and the run adds up both passes.

use super::super::outcomes::Counts;
use super::super::pass::{self, Judged};
use super::*;

const WIRE_CMD: &str = "cargo mutants --package clauseo-protocol --features \
                        clauseo-protocol/specta --file crates/protocol/src/wire.rs --in-diff \
                        target/xtask/mutants.diff --output target/xtask/wire --jobs 2";
const MAIN_LINE: &str = "mutants, everything but crates/protocol/src/wire.rs: ";
const WIRE_LINE: &str = "mutants, crates/protocol/src/wire.rs with protocol's specta feature: ";

/// A diff that changes wire.rs and xtask's main.rs.
fn wire_diff() -> String {
    format!("diff --git a/crates/protocol/src/wire.rs b/crates/protocol/src/wire.rs\n{DIFF_TEXT}")
}

/// `check_runs` on `wire_diff()`, with git listing nothing and cargo-mutants installed.
fn two_passes(name: &str, main: &Run, wire: &Run) -> (String, bool, Vec<String>) {
    check_runs(
        name,
        Ok(wire_diff()),
        &Git::Lists(""),
        Some("cargo-mutants 27.1.0"),
        main,
        Some(wire),
    )
}

/// The header and summary lines of `printed`: the lines that start with `==>` or `mutants`.
fn summary_lines(printed: &str) -> Vec<&str> {
    printed
        .lines()
        .filter(|line| line.starts_with("==>") || line.starts_with("mutants"))
        .collect()
}

#[test]
fn a_diff_that_does_not_change_wire_rs_runs_one_pass() {
    // wire.rs named in a changed line, and a file whose name only ends like it, are no change
    // to wire.rs.
    let diff = format!(
        "{DIFF_TEXT}+// see crates/protocol/src/wire.rs\n\
         diff --git a/crates/protocol/src/wire.rs.orig b/crates/protocol/src/wire.rs.orig\n"
    );
    let outcomes = lab(
        &[baseline("Success"), mutant("CaughtMutant", 1, 1, "x")],
        true,
    );
    let (printed, passed, ran) = check_runs(
        "mutants-one-pass",
        Ok(diff),
        &Git::Lists(""),
        Some("cargo-mutants 27.1.0"),
        &Run::Writes(outcomes, true),
        None,
    );
    assert!(passed, "{printed}");
    assert_eq!(ran, [UNTRACKED_CMD, "cargo mutants --version", CMD]);
    assert_eq!(
        summary_lines(&printed),
        [
            format!("==> mutants: {CMD}"),
            "mutants ok: 1 mutant tested: 0 missed, 1 caught, 0 timed out, 0 unviable".to_owned(),
        ]
    );
}

#[test]
fn a_diff_that_changes_wire_rs_runs_a_second_pass_on_it_with_specta_and_adds_them_up() {
    let main = lab(
        &[
            baseline("Success"),
            mutant("CaughtMutant", 106, 5, "replace double -> u32 with 0"),
            mutant(
                "Unviable",
                130,
                5,
                "replace parse -> Lab with Default::default()",
            ),
        ],
        true,
    );
    let wire = lab(
        &[
            baseline("Success"),
            mutant("CaughtMutant", 10, 5, "replace == with !="),
            mutant("Timeout", 20, 9, "replace += with -= in count"),
            mutant(
                "Unviable",
                30,
                5,
                "replace hash -> u32 with Default::default()",
            ),
        ],
        true,
    );
    let (printed, passed, ran) = two_passes(
        "mutants-two-passes",
        &Run::Writes(main, true),
        &Run::Writes(wire, false),
    );
    assert!(passed, "{printed}");
    assert_eq!(
        ran,
        [UNTRACKED_CMD, "cargo mutants --version", CMD, WIRE_CMD]
    );
    assert_eq!(
        summary_lines(&printed),
        [
            format!("==> mutants: {CMD}"),
            format!("{MAIN_LINE}2 mutants tested: 0 missed, 1 caught, 0 timed out, 1 unviable"),
            format!("==> mutants: {WIRE_CMD}"),
            format!("{WIRE_LINE}3 mutants tested: 0 missed, 1 caught, 1 timed out, 1 unviable"),
            "mutants ok: 5 mutants tested: 0 missed, 2 caught, 1 timed out, 2 unviable, in both \
             passes"
                .to_owned(),
        ]
    );
    // Each pass's notes point at its own output folder.
    assert!(
        printed.contains("target/xtask/mutants.out/unviable.txt"),
        "{printed}"
    );
    assert!(
        printed.contains("target/xtask/wire/mutants.out/unviable.txt lists them"),
        "{printed}"
    );
}

#[test]
fn a_missed_mutant_in_either_pass_fails_the_run() {
    // cargo-mutants exits non-zero on a missed mutant and zero when all were caught.
    let missed = || {
        let outcomes = lab(
            &[
                baseline("Success"),
                mutant("MissedMutant", 106, 5, "replace double -> u32 with 0"),
            ],
            true,
        );
        Run::Writes(outcomes, false)
    };
    let caught = || {
        let outcomes = lab(
            &[baseline("Success"), mutant("CaughtMutant", 1, 1, "x")],
            true,
        );
        Run::Writes(outcomes, true)
    };
    let violation = format!(
        "xtask/src/main.rs:106: missed mutant: no test fails on `replace double -> u32 with 0`\n\
         {MISSED_FIX}\n"
    );
    for (name, main, wire, main_line, wire_line) in [
        (
            "mutants-missed-main",
            missed(),
            caught(),
            "1 missed, 0 caught",
            "0 missed, 1 caught",
        ),
        (
            "mutants-missed-wire",
            caught(),
            missed(),
            "0 missed, 1 caught",
            "1 missed, 0 caught",
        ),
    ] {
        let (printed, passed, ran) = two_passes(name, &main, &wire);
        assert!(!passed, "{printed}");
        assert_eq!(ran.len(), 4, "{ran:?}");
        assert!(printed.contains(&violation), "{printed}");
        assert_eq!(
            summary_lines(&printed),
            [
                format!("==> mutants: {CMD}"),
                format!("{MAIN_LINE}1 mutant tested: {main_line}, 0 timed out, 0 unviable"),
                format!("==> mutants: {WIRE_CMD}"),
                format!("{WIRE_LINE}1 mutant tested: {wire_line}, 0 timed out, 0 unviable"),
                "mutants FAILED: 2 mutants tested: 1 missed, 1 caught, 0 timed out, 0 unviable, \
                 in both passes"
                    .to_owned(),
            ]
        );
    }
}

#[test]
fn a_main_pass_with_no_mutant_still_runs_the_wire_pass() {
    let wire = lab(
        &[baseline("Success"), mutant("CaughtMutant", 1, 1, "x")],
        true,
    );
    let (printed, passed, _) = two_passes(
        "mutants-wire-only",
        &Run::Nothing(true, "INFO No mutants to filter\n"),
        &Run::Writes(wire, true),
    );
    assert!(passed, "{printed}");
    assert_eq!(
        summary_lines(&printed),
        [
            format!("==> mutants: {CMD}"),
            format!("{MAIN_LINE}cargo-mutants found no mutant in the changed lines"),
            format!("==> mutants: {WIRE_CMD}"),
            format!("{WIRE_LINE}1 mutant tested: 0 missed, 1 caught, 0 timed out, 0 unviable"),
            "mutants ok: 1 mutant tested: 0 missed, 1 caught, 0 timed out, 0 unviable, in both \
             passes"
                .to_owned(),
        ]
    );
}

#[test]
fn a_main_pass_that_failed_before_any_mutant_fails_the_run_after_the_wire_pass() {
    let (printed, passed, ran) = two_passes(
        "mutants-main-early",
        &Run::Nothing(false, "ERROR Failed to parse diff: bad hunk\n"),
        &Run::Nothing(true, "INFO No mutants to filter\n"),
    );
    assert!(!passed, "{printed}");
    assert_eq!(ran.len(), 4, "{ran:?}");
    assert_eq!(
        summary_lines(&printed),
        [
            format!("==> mutants: {CMD}"),
            format!("{MAIN_LINE}it failed before it tested any mutant"),
            format!("==> mutants: {WIRE_CMD}"),
            format!("{WIRE_LINE}cargo-mutants found no mutant in the changed lines"),
            "mutants FAILED: 0 mutants tested: 0 missed, 0 caught, 0 timed out, 0 unviable, in \
             both passes"
                .to_owned(),
        ]
    );
}

#[test]
fn a_wire_pass_that_runs_past_what_is_left_of_the_cap_fails_with_split_the_change() {
    let main = lab(
        &[baseline("Success"), mutant("CaughtMutant", 1, 1, "x")],
        true,
    );
    let (printed, passed, _) =
        two_passes("mutants-wire-cap", &Run::Writes(main, true), &Run::TimesOut);
    assert!(!passed, "{printed}");
    assert!(
        printed.contains(&format!("`{WIRE_CMD}` did not finish within ")),
        "{printed}"
    );
    assert!(printed.ends_with(
        "so it was stopped\n    fix: split the change: cargo xtask mutants caps a run at 20 \
         minutes\n"
    ));
}

#[test]
fn a_main_pass_stopped_at_the_cap_runs_no_wire_pass() {
    let (printed, passed, ran) = two_passes("mutants-main-cap", &Run::TimesOut, &Run::TimesOut);
    assert!(!passed, "{printed}");
    assert_eq!(ran, [UNTRACKED_CMD, "cargo mutants --version", CMD]);
}

#[test]
fn a_main_pass_that_used_the_whole_cap_leaves_the_wire_pass_unrun_and_fails() {
    let root = Scratch::new("mutants-no-time-left");
    let mut out = Printer::new(Vec::new(), false);
    let mut exec = |cmd: &Cmd, _: &mut dyn FnMut(&[u8])| -> io::Result<Ran> {
        panic!("nothing runs once the cap is spent: `{cmd}`")
    };
    let first = Judged::NoMutant;
    let spent = MUTANTS_TIMEOUT + Duration::from_secs(3);
    let passed = run_wire(&mut out, &root, &root, spent, &mut exec, &first);
    assert!(!passed);
    assert_eq!(
        String::from_utf8(out.into_inner()).expect("utf-8"),
        "the first pass used the whole 20 min cap, so the mutants of crates/protocol/src/wire.rs \
         were not tested\n    fix: split the change: cargo xtask mutants caps a run at 20 \
         minutes\n"
    );
}

#[test]
fn the_wire_pass_gets_the_whole_seconds_left_of_the_cap() {
    let secs = Duration::from_secs;
    assert_eq!(pass::remaining(Duration::ZERO), Some(MUTANTS_TIMEOUT));
    assert_eq!(pass::remaining(secs(300)), Some(secs(900)));
    assert_eq!(
        pass::remaining(secs(300) + Duration::from_millis(700)),
        Some(secs(899))
    );
    assert_eq!(pass::remaining(secs(1199)), Some(secs(1)));
    assert_eq!(pass::remaining(Duration::from_millis(1_199_500)), None);
    assert_eq!(pass::remaining(MUTANTS_TIMEOUT), None);
    assert_eq!(pass::remaining(secs(1500)), None);
}

#[test]
fn counts_add_field_by_field() {
    let mut counts = Counts {
        tested: 20,
        missed: 1,
        caught: 12,
        timeout: 2,
        unviable: 4,
        other: 1,
    };
    counts.add(&Counts {
        tested: 9,
        missed: 2,
        caught: 3,
        timeout: 1,
        unviable: 2,
        other: 1,
    });
    assert_eq!(
        counts,
        Counts {
            tested: 29,
            missed: 3,
            caught: 15,
            timeout: 3,
            unviable: 6,
            other: 2,
        }
    );
}

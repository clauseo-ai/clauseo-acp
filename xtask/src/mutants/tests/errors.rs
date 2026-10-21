//! Tests of the runs that fail before cargo-mutants tests a mutant: no cargo-mutants, no diff,
//! no start, and outcomes that cannot be read.

use super::*;

#[test]
fn a_missing_or_other_cargo_mutants_fails_with_setup() {
    let (printed, passed, ran) = check(
        "mutants-missing",
        Ok(DIFF_TEXT.to_owned()),
        None,
        &Run::TimesOut,
    );
    assert!(!passed);
    assert_eq!(ran, ["cargo mutants --version"]);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             cargo-mutants 27.1.0 is not installed: `cargo mutants --version` failed: error: no \
             such command: `mutants`\n    fix: run `cargo xtask setup`, which installs \
             cargo-mutants 27.1.0, the version xtask/src/setup.rs pins\n"
        )
    );
    let (printed, _, _) = check(
        "mutants-other",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 26.2.0"),
        &Run::TimesOut,
    );
    assert!(
        printed.contains(
            "cargo-mutants 27.1.0 is not installed: `cargo mutants --version` printed \
             `cargo-mutants 26.2.0`\n"
        ),
        "{printed}"
    );
}

#[test]
fn a_git_error_fails_with_the_command_to_run() {
    let error = GitError::Failed {
        command: "git merge-base HEAD origin/main".into(),
        stderr: "fatal: Not a valid object name origin/main".into(),
    };
    let (printed, passed, ran) = check(
        "mutants-git",
        Err(error),
        Some("cargo-mutants 27.1.0"),
        &Run::TimesOut,
    );
    assert!(!passed);
    assert_eq!(ran, Vec::<String>::new());
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             could not diff against the merge-base with origin/main: `git merge-base HEAD \
             origin/main` failed: fatal: Not a valid object name origin/main\n    fix: run `git \
             merge-base HEAD origin/main` by hand to see why it fails; `git fetch origin main` \
             fetches a missing origin/main\n"
        )
    );
}

#[test]
fn a_run_that_cannot_start_fails_with_its_error() {
    let (printed, passed, _) = check(
        "mutants-no-start",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::CannotStart,
    );
    assert!(!passed);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\ncould not run `{CMD}`: no cargo\n    fix: run `{CMD}` by hand \
             to see what goes wrong\n"
        )
    );
}

#[test]
fn outcomes_that_cannot_be_read_fail_even_after_a_clean_exit() {
    let (printed, passed, _) = check(
        "mutants-unreadable",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::FolderForOutcomes,
    );
    assert!(!passed);
    assert!(
        printed.starts_with(&format!(
            "==> mutants: {CMD}\ncould not read target/xtask/mutants.out/outcomes.json: "
        )),
        "{printed}"
    );
}

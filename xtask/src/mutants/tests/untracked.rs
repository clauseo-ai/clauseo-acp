//! Tests of the untracked-file check: a `.rs` file git does not track is not in the diff, so
//! cargo-mutants would mutate none of it; the run fails before it starts, naming each file.

use super::*;

#[test]
fn untracked_rs_files_fail_the_run_before_it_starts_naming_each_file() {
    let (printed, passed, ran) = check_with(
        "mutants-untracked",
        Ok(DIFF_TEXT.to_owned()),
        &Git::Lists("xtask/src/new.rs\0xtask/src/new/tests.rs\0"),
        Some("cargo-mutants 27.1.0"),
        &Run::TimesOut,
    );
    assert!(!passed);
    assert_eq!(ran, [UNTRACKED_CMD]);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             xtask/src/new.rs is untracked, so cargo-mutants would mutate none of it\n    fix: \
             git add xtask/src/new.rs (or git add -N xtask/src/new.rs) so cargo-mutants sees \
             it, or ignore it in .gitignore\n\
             xtask/src/new/tests.rs is untracked, so cargo-mutants would mutate none of it\n    \
             fix: git add xtask/src/new/tests.rs (or git add -N xtask/src/new/tests.rs) so \
             cargo-mutants sees it, or ignore it in .gitignore\n"
        )
    );
}

#[test]
fn an_untracked_file_fails_even_when_nothing_tracked_changed() {
    let (printed, passed, ran) = check_with(
        "mutants-untracked-empty",
        Ok(String::new()),
        &Git::Lists("xtask/src/new.rs\0"),
        Some("cargo-mutants 27.1.0"),
        &Run::TimesOut,
    );
    assert!(!passed, "{printed}");
    assert_eq!(ran, [UNTRACKED_CMD]);
    assert!(!printed.contains("mutants ok"), "{printed}");
}

#[test]
fn a_git_that_cannot_list_the_untracked_files_fails_with_why() {
    let (printed, passed, ran) = check_with(
        "mutants-untracked-git-fails",
        Ok(DIFF_TEXT.to_owned()),
        &Git::Fails("fatal: not a git repository\n"),
        Some("cargo-mutants 27.1.0"),
        &Run::TimesOut,
    );
    assert!(!passed);
    assert_eq!(ran, [UNTRACKED_CMD]);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             could not list the untracked .rs files: `{UNTRACKED_CMD}` failed: fatal: not a git \
             repository\n    fix: run `{UNTRACKED_CMD}` by hand to see what goes wrong\n"
        )
    );
    let (printed, passed, _) = check_with(
        "mutants-untracked-no-git",
        Ok(DIFF_TEXT.to_owned()),
        &Git::CannotStart,
        Some("cargo-mutants 27.1.0"),
        &Run::TimesOut,
    );
    assert!(!passed);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             could not list the untracked .rs files: could not run git: no git\n    fix: install \
             git, or put it on PATH\n"
        )
    );
}

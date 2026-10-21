//! Cases of the pins check run with made-up `BINARIES` (`repos::BINARIES`), through `inspect`,
//! the whole check but for the pins it is handed. They were planted cases; the planted harness
//! runs `run`, which reads the real pins, and their expected text would then repeat the real
//! lefthook version, a second home for it that the pins rule forbids.

use super::repos::{self, check_in};
use crate::checks::planted::Repo;

/// Runs the check with the made-up pins on `repo` and asserts the count and printed failures.
fn assert_found(repo: &Repo, inspects: usize, expected: &[&str]) {
    let found = check_in(repo.ctx());
    let printed: Vec<String> = found.violations.iter().map(ToString::to_string).collect();
    assert_eq!(
        (found.inspected, printed),
        (
            inspects,
            expected.iter().map(|line| (*line).to_owned()).collect()
        )
    );
    assert!(found.failures.is_empty(), "{:?}", found.failures);
}

/// A comment that names another lefthook fails at its line.
#[test]
fn lefthook_comment_differs() {
    assert_found(
        &repos::lefthook_comment_differs(),
        2,
        &[
            "lefthook.yml:3: the comment names lefthook 9.8.6, not 9.8.7, the version `BINARIES` in \
     xtask/src/setup.rs pins\n    fix: check that the comment holds for lefthook 9.8.7, then \
     make it name 9.8.7, the version `BINARIES` in xtask/src/setup.rs pins",
        ],
    );
}

/// A comment after a value counts, in any case, after a quoted string too.
#[test]
fn lefthook_comment_after_a_value() {
    assert_found(
        &repos::lefthook_comment_after_a_value(),
        2,
        &[
            "lefthook.yml:2: the comment names lefthook 10.0, not 9.8.7, the version `BINARIES` in \
     xtask/src/setup.rs pins\n    fix: check that the comment holds for lefthook 9.8.7, then \
     make it name 9.8.7, the version `BINARIES` in xtask/src/setup.rs pins",
            "lefthook.yml:4: the comment names lefthook 9.0.0, not 9.8.7, the version `BINARIES` in \
     xtask/src/setup.rs pins\n    fix: check that the comment holds for lefthook 9.8.7, then \
     make it name 9.8.7, the version `BINARIES` in xtask/src/setup.rs pins",
        ],
    );
}

/// The `lefthook@<version>` form, in upper case, after a flow sequence, fails at its line.
#[test]
fn lefthook_comment_at_form() {
    assert_found(
        &repos::lefthook_comment_at_form(),
        2,
        &[
            "lefthook.yml:2: the comment names lefthook 9.8.6, not 9.8.7, the version `BINARIES` \
             in xtask/src/setup.rs pins\n    fix: check that the comment holds for lefthook \
             9.8.7, then make it name 9.8.7, the version `BINARIES` in xtask/src/setup.rs pins",
        ],
    );
}

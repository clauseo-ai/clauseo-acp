//! Cases of the pins check run with made-up `TOOLS` and `BINARIES` (`repos::TOOLS`,
//! `repos::BINARIES`), through `inspect`, the whole check but for the pins it is handed. They
//! were planted cases; the planted harness runs `run`, which reads the real pins, and their
//! expected text would then repeat the real lefthook and cargo tool versions, a second home for
//! each that the pins rule forbids.

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

/// A pinned version, or a major.minor, in a run step fails at its line.
#[test]
fn versions_in_run_steps() {
    assert_found(
        &repos::versions_in_run_steps(),
        2,
        &[
            ".github/workflows/ci.yml:4: `pnpm@7.8.9` names pnpm 7.8.9; the one home of the pnpm \
             version is `packageManager` in ui/package.json\n    fix: remove the version from this \
             file: pnpm/action-setup with `package_json_file: ui/package.json` installs the pnpm \
             `packageManager` names",
            ".github/workflows/ci.yml:7: `pnpm@7.8` names pnpm 7.8; the one home of the pnpm \
             version is `packageManager` in ui/package.json\n    fix: remove the version from this \
             file: pnpm/action-setup with `package_json_file: ui/package.json` installs the pnpm \
             `packageManager` names",
            ".github/workflows/ci.yml:8: `rustup toolchain install 3.4.5` names Rust toolchain \
             3.4.5; the one home of the Rust version is `channel` in rust-toolchain.toml\n    fix: \
             remove the version from this file: a `run: rustup toolchain install` step with no \
             argument installs the toolchain rust-toolchain.toml names",
            ".github/workflows/ci.yml:9: `cargo-fake@5.6.7` names cargo-fake 5.6.7; the one home \
             of the cargo-fake version is `TOOLS` in xtask/src/setup.rs\n    fix: remove the \
             version from this file: `cargo xtask setup --binaries` installs the versions \
             xtask/src/setup.rs pins",
            ".github/workflows/ci.yml:9: `lefthook@9.8.7` names lefthook 9.8.7; the one home of \
             the lefthook version is `BINARIES` in xtask/src/setup.rs\n    fix: remove the version \
             from this file: `cargo xtask setup --binaries` installs the versions \
             xtask/src/setup.rs pins",
        ],
    );
}

/// A version in a container image, a docker image or an install action's tool input fails;
/// the Node major.minor alone in an env value names no tool and passes.
#[test]
fn versions_in_other_values() {
    assert_found(
        &repos::versions_in_other_values(),
        2,
        &[
            ".github/workflows/ci.yml:5: `rust:3.4` names Rust toolchain 3.4; the one home of the \
             Rust version is `channel` in rust-toolchain.toml\n    fix: remove the version from \
             this file: a `run: rustup toolchain install` step with no argument installs the \
             toolchain rust-toolchain.toml names",
            ".github/workflows/ci.yml:7: `rust:3.4.5` names Rust toolchain 3.4.5; the one home of \
             the Rust version is `channel` in rust-toolchain.toml\n    fix: remove the version \
             from this file: a `run: rustup toolchain install` step with no argument installs the \
             toolchain rust-toolchain.toml names",
            ".github/workflows/ci.yml:10: `cargo-fake@5.6.7` names cargo-fake 5.6.7; the one home \
             of the cargo-fake version is `TOOLS` in xtask/src/setup.rs\n    fix: remove the \
             version from this file: `cargo xtask setup --binaries` installs the versions \
             xtask/src/setup.rs pins",
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

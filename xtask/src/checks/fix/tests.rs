//! Pins the text of every `checks::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::NothingInspected { .. } => {
            "budgets looks for budget rows in budgets.toml at the repo root; restore them, or, if \
             they moved, update the consts xtask/src/checks/budgets.rs builds that place from"
        },
        Fix::Metadata => {
            "fix the error quoted above so that `cargo metadata` succeeds; this check reads the \
             workspace members from its output"
        },
        Fix::DesktopNotMember => {
            "restore crates/desktop or update xtask/src/checks: these checks guard its \
             capabilities, plugins and Tauri config"
        },
        Fix::GitNotStarted { .. } => {
            "install git and put it on PATH: docs lists the files it reads with `git ls-files`"
        },
        Fix::GitFailed { .. } => {
            "run the check inside a git checkout, or fix what git reports: docs lists the files it \
             reads with `git ls-files`"
        },
        Fix::GitTimedOut { .. } => {
            "run that git command by hand to see why it does not finish: docs lists the files it \
             reads with `git ls-files`"
        },
        Fix::NotUtf8 => "make the file readable as UTF-8 text",
        Fix::Unreadable => {
            "fix what the error says so the check can read it: restore or replace the file or \
             folder"
        },
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::NothingInspected {
            name: "budgets",
            many: "budget rows",
            place: "in budgets.toml at the repo root".to_owned(),
            source: "xtask/src/checks/budgets.rs",
        },
        Fix::Metadata,
        Fix::DesktopNotMember,
        Fix::GitNotStarted {
            check: "docs".to_owned(),
        },
        Fix::GitFailed {
            check: "docs".to_owned(),
        },
        Fix::GitTimedOut {
            check: "docs".to_owned(),
        },
        Fix::NotUtf8,
        Fix::Unreadable,
    ];
    assert_texts(&samples, pinned, ARMS);
}

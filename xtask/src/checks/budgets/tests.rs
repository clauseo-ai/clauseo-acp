//! Tests of the budgets check. The planted cases are in `tests/planted.rs` and their builders in
//! `tests/repos.rs`; the lines below are the expected failures that differ by OS or that several
//! cases share.

pub(super) mod planted;
mod repos;

use super::*;
use crate::budgets::Fix;
use crate::checks::planted::{fails, failure};

#[test]
fn rows_with_problems_still_count() {
    let repo = repos::budgets_file(
        "count",
        format!(
            "{}\n{}",
            repos::row("limit = -1.0"),
            repos::row("limit = 1.0")
        ),
    );
    let found = run(repo.ctx());
    assert_eq!(found.inspected, 2);
    assert_eq!(found.violations.len(), 2);
}

/// The zero-inspected failure of the budgets check, as it prints.
fn nothing_inspected() -> String {
    failure(
        "0 budget rows inspected in budgets.toml at the repo root",
        crate::checks::Fix::NothingInspected {
            name: "budgets",
            many: "budget rows",
            place: "in budgets.toml at the repo root".to_owned(),
            source: "xtask/src/checks/budgets.rs".to_owned(),
        },
    )
}

/// What the `missing_file` case prints: the read error, in the OS's words, and the zero line.
fn missing_file() -> Vec<String> {
    #[cfg(not(windows))]
    let error = "No such file or directory (os error 2)";
    #[cfg(windows)]
    let error = "The system cannot find the file specified. (os error 2)";
    vec![
        fails(
            "budgets.toml:1",
            &format!("cannot read budgets.toml: {error}"),
            Fix::Restore,
        ),
        nothing_inspected(),
    ]
}

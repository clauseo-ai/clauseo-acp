//! Tests of the budgets check; the planted cases are in `tests/planted.rs`.

pub(super) mod planted;

use super::*;
use crate::checks::planted::Repo;
use crate::scratch::Scratch;

/// A valid row whose limit line (line 4) is `limit`.
fn row(limit: &str) -> String {
    format!(
        "[[budget]]\nid = \"window_visible\"\nmetric = \"window visible\"\n{limit}\n\
         unit = \"ms\"\ngate = \"alert\"\nmeasured_by = \"marks\"\nstatus = \"unmeasured\"\n\
         source = \"doc 15\"\n"
    )
}

/// A scratch repo whose `budgets.toml` is `text`.
fn run_on(case: &str, text: impl AsRef<[u8]>) -> Repo {
    let dir = Scratch::new(&format!("planted-budgets-{case}"));
    Scratch::write(dir.join(budgets::FILE), text).expect("write budgets.toml");
    Repo::new(dir)
}

pub(super) fn negative_limit() -> Repo {
    run_on("negative-limit", row("limit = -1.0"))
}

pub(super) fn not_toml() -> Repo {
    run_on("not-toml", "[[budget]\nid = \n")
}

pub(super) fn not_utf8() -> Repo {
    run_on("not-utf8", b"[[budget]]\nid = \"\xff\"\n")
}

pub(super) fn valid_row() -> Repo {
    run_on("valid-row", row("limit = 250.0"))
}

#[test]
fn rows_with_problems_still_count() {
    let repo = run_on(
        "count",
        format!("{}\n{}", row("limit = -1.0"), row("limit = 1.0")),
    );
    let found = run(repo.ctx());
    assert_eq!(found.inspected, 2);
    assert_eq!(found.violations.len(), 2);
}

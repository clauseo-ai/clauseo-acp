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

/// The valid row with `from` replaced by `to`.
fn changed(from: &str, to: &str) -> String {
    let valid = row("limit = 250.0");
    assert!(valid.contains(from), "the row holds {from:?}");
    valid.replacen(from, to, 1)
}

pub(super) fn missing_key() -> Repo {
    run_on("missing-key", changed("source = \"doc 15\"\n", ""))
}

pub(super) fn missing_id() -> Repo {
    run_on("missing-id", changed("id = \"window_visible\"\n", ""))
}

pub(super) fn unknown_key() -> Repo {
    let text = changed(
        "source = \"doc 15\"\n",
        "source = \"doc 15\"\ncolour = \"red\"\n",
    );
    run_on("unknown-key", text)
}

pub(super) fn misspelt_table() -> Repo {
    run_on("misspelt-table", changed("[[budget]]", "[[budgets]]"))
}

pub(super) fn key_before_first_table() -> Repo {
    run_on(
        "key-before-table",
        format!("limit = 1.0\n{}", row("limit = 250.0")),
    )
}

pub(super) fn string_limit() -> Repo {
    run_on("string-limit", row("limit = \"250\""))
}

pub(super) fn number_metric() -> Repo {
    run_on(
        "number-metric",
        changed("metric = \"window visible\"", "metric = 4"),
    )
}

pub(super) fn unit_outside_set() -> Repo {
    run_on(
        "unit-outside-set",
        changed("unit = \"ms\"", "unit = \"seconds\""),
    )
}

pub(super) fn key_given_twice() -> Repo {
    let text = changed("source = \"doc 15\"\n", "source = \"a\"\nsource = \"b\"\n");
    run_on("key-twice", text)
}

pub(super) fn infinite_limit() -> Repo {
    run_on("infinite-limit", row("limit = inf"))
}

pub(super) fn nan_limit() -> Repo {
    run_on("nan-limit", row("limit = nan"))
}

pub(super) fn id_not_snake_case() -> Repo {
    run_on(
        "id-not-snake-case",
        changed("id = \"window_visible\"", "id = \"Window-Visible\""),
    )
}

pub(super) fn duplicate_id() -> Repo {
    let valid = row("limit = 250.0");
    run_on("duplicate-id", format!("{valid}\n{valid}"))
}

pub(super) fn measured_on_shape() -> Repo {
    let text = changed(
        "source = \"doc 15\"\n",
        "source = \"doc 15\"\nmeasured = 2.0\nmeasured_on = \"Mac Book\"\n",
    );
    run_on("measured-on-shape", text)
}

pub(super) fn measured_without_on() -> Repo {
    let text = changed(
        "source = \"doc 15\"\n",
        "source = \"doc 15\"\nmeasured = 2.0\n",
    );
    run_on("measured-without-on", text)
}

pub(super) fn on_without_measured() -> Repo {
    let text = changed(
        "source = \"doc 15\"\n",
        "source = \"doc 15\"\nmeasured_on = \"macos-arm64\"\n",
    );
    run_on("on-without-measured", text)
}

pub(super) fn measured_without_value() -> Repo {
    run_on(
        "measured-without-value",
        changed("status = \"unmeasured\"", "status = \"measured\""),
    )
}

pub(super) fn no_tables() -> Repo {
    run_on("no-tables", "# just a comment\n")
}

pub(super) fn missing_file() -> Repo {
    Repo::new(Scratch::new("planted-budgets-missing-file"))
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

/// What the `missing_file` case prints: the read error, in the OS's words, and the zero line.
const MISSING_FILE: &[&str] = &[
    #[cfg(not(windows))]
    "budgets.toml:1: cannot read budgets.toml: No such file or directory (os error 2)\n    \
     fix: restore budgets.toml at the repo root",
    #[cfg(windows)]
    "budgets.toml:1: cannot read budgets.toml: The system cannot find the file specified. \
     (os error 2)\n    fix: restore budgets.toml at the repo root",
    "0 budget rows inspected in budgets.toml at the repo root\n    \
     fix: budgets looks for budget rows in budgets.toml at the repo root; restore them, or, if \
     they moved, update the consts xtask/src/checks/budgets.rs builds that place from",
];

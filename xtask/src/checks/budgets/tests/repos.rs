//! The builders of the budgets planted cases: a scratch repo whose `budgets.toml` is one valid
//! row (`row`) with one thing changed.

use crate::budgets;
use crate::checks::planted::Repo;

/// A valid row whose limit line (line 4) is `limit`.
pub(super) fn row(limit: &str) -> String {
    format!(
        "[[budget]]\nid = \"window_visible\"\nmetric = \"window visible\"\n{limit}\n\
         unit = \"ms\"\ngate = \"alert\"\nmeasured_by = \"marks\"\nstatus = \"unmeasured\"\n\
         source = \"doc 15\"\n"
    )
}

/// A scratch repo whose `budgets.toml` is `text`.
pub(super) fn budgets_file(case: &str, text: impl AsRef<[u8]>) -> Repo {
    Repo::files(&format!("budgets-{case}"), &[(budgets::FILE, text)])
}

/// The valid row with `from` replaced by `to`.
fn changed(from: &str, to: &str) -> String {
    let valid = row("limit = 250.0");
    assert!(valid.contains(from), "the row holds {from:?}");
    valid.replacen(from, to, 1)
}

pub(super) fn negative_limit() -> Repo {
    budgets_file("negative-limit", row("limit = -1.0"))
}

pub(super) fn not_toml() -> Repo {
    budgets_file("not-toml", "[[budget]\nid = \n")
}

pub(super) fn not_utf8() -> Repo {
    budgets_file("not-utf8", b"[[budget]]\nid = \"\xff\"\n")
}

pub(super) fn valid_row() -> Repo {
    budgets_file("valid-row", row("limit = 250.0"))
}

pub(super) fn missing_key() -> Repo {
    budgets_file("missing-key", changed("source = \"doc 15\"\n", ""))
}

pub(super) fn missing_id() -> Repo {
    budgets_file("missing-id", changed("id = \"window_visible\"\n", ""))
}

pub(super) fn unknown_key() -> Repo {
    let text = changed(
        "source = \"doc 15\"\n",
        "source = \"doc 15\"\ncolour = \"red\"\n",
    );
    budgets_file("unknown-key", text)
}

pub(super) fn misspelt_table() -> Repo {
    budgets_file("misspelt-table", changed("[[budget]]", "[[budgets]]"))
}

pub(super) fn key_before_first_table() -> Repo {
    budgets_file(
        "key-before-table",
        format!("limit = 1.0\n{}", row("limit = 250.0")),
    )
}

pub(super) fn string_limit() -> Repo {
    budgets_file("string-limit", row("limit = \"250\""))
}

pub(super) fn number_metric() -> Repo {
    budgets_file(
        "number-metric",
        changed("metric = \"window visible\"", "metric = 4"),
    )
}

pub(super) fn unit_outside_set() -> Repo {
    budgets_file(
        "unit-outside-set",
        changed("unit = \"ms\"", "unit = \"seconds\""),
    )
}

pub(super) fn key_given_twice() -> Repo {
    let text = changed("source = \"doc 15\"\n", "source = \"a\"\nsource = \"b\"\n");
    budgets_file("key-twice", text)
}

pub(super) fn infinite_limit() -> Repo {
    budgets_file("infinite-limit", row("limit = inf"))
}

pub(super) fn nan_limit() -> Repo {
    budgets_file("nan-limit", row("limit = nan"))
}

pub(super) fn id_not_snake_case() -> Repo {
    budgets_file(
        "id-not-snake-case",
        changed("id = \"window_visible\"", "id = \"Window-Visible\""),
    )
}

pub(super) fn duplicate_id() -> Repo {
    let valid = row("limit = 250.0");
    budgets_file("duplicate-id", format!("{valid}\n{valid}"))
}

pub(super) fn measured_on_shape() -> Repo {
    let text = changed(
        "source = \"doc 15\"\n",
        "source = \"doc 15\"\nmeasured = 2.0\nmeasured_on = \"Mac Book\"\n",
    );
    budgets_file("measured-on-shape", text)
}

pub(super) fn measured_without_on() -> Repo {
    let text = changed(
        "source = \"doc 15\"\n",
        "source = \"doc 15\"\nmeasured = 2.0\n",
    );
    budgets_file("measured-without-on", text)
}

pub(super) fn on_without_measured() -> Repo {
    let text = changed(
        "source = \"doc 15\"\n",
        "source = \"doc 15\"\nmeasured_on = \"macos-arm64\"\n",
    );
    budgets_file("on-without-measured", text)
}

pub(super) fn measured_without_value() -> Repo {
    budgets_file(
        "measured-without-value",
        changed("status = \"unmeasured\"", "status = \"measured\""),
    )
}

pub(super) fn no_tables() -> Repo {
    budgets_file("no-tables", "# just a comment\n")
}

pub(super) fn missing_file() -> Repo {
    Repo::files::<&str>("budgets-missing-file", &[])
}

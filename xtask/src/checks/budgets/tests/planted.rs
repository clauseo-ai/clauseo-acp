//! The planted cases of the budgets check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A row whose limit is below zero fails at the limit's line.
    negative_limit: FailsWith(&[
        "budgets.toml:4: budget window_visible: limit must be zero or more\n    \
         fix: set limit to zero or more, in the row's unit",
    ]) => super::negative_limit,
    /// A file that is not TOML fails, and the check stopped on it: no row was looked at.
    not_toml: FailsWith(&[
        "budgets.toml:1: budgets.toml is not valid: unclosed array table, expected `]`\n    \
         fix: fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
         of budgets.toml",
        "0 budget rows inspected in budgets.toml at the repo root\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::not_toml,
    /// A file that is not UTF-8 is there but cannot be read: the check stopped on it.
    not_utf8: FailsWith(&[
        "budgets.toml:1: could not read `budgets.toml`: stream did not contain valid UTF-8\n    \
         fix: make the file readable as UTF-8 text",
        "0 budget rows inspected in budgets.toml at the repo root\n    \
         fix: the check stopped before it could look; fix the failure above first",
    ]) => super::not_utf8,
    /// A row with every required key passes.
    valid_row: Passes => super::valid_row,
}

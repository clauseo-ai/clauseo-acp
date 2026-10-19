//! The planted cases of the budgets check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A row whose limit is below zero fails at the limit's line.
    negative_limit: FailsWith(&[
        "budgets.toml:4: budget window_visible: limit must be zero or more\n    \
         fix: set limit to zero or more, in the row's unit",
    ]) => super::negative_limit,
    /// A file that is not TOML fails.
    not_toml: Fails => super::not_toml,
    /// A row with every required key passes.
    valid_row: Passes => super::valid_row,
}

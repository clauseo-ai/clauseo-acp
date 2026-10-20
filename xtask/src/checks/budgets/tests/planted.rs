//! The planted cases of the budgets check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A row whose limit is below zero fails at the limit's line.
    negative_limit: FailsWith(&[
        "budgets.toml:4: budget window_visible: limit must be zero or more\n    \
         fix: set limit to zero or more, in the row's unit",
    ]), inspects 1 => super::negative_limit,
    /// A file that is not TOML fails, and the check stopped on it: no row was looked at.
    not_toml: FailsWith(&[
        "budgets.toml:1: budgets.toml is not valid: unclosed array table, expected `]`\n    \
         fix: fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
         of budgets.toml",
    ]), inspects 0 => super::not_toml,
    /// A file that is not UTF-8 is there but cannot be read: the check stopped on it.
    not_utf8: FailsWith(&[
        "budgets.toml:1: could not read `budgets.toml`: stream did not contain valid UTF-8\n    \
         fix: make the file readable as UTF-8 text",
    ]), inspects 0 => super::not_utf8,
    /// A row with every required key passes.
    valid_row: Passes, inspects 1 => super::valid_row,
    /// A row missing a required key fails at its header, naming the key.
    missing_key: FailsWith(&[
        "budgets.toml:1: budget window_visible has no source\n    \
         fix: add source = \"the plan section it comes from\"",
    ]), inspects 1 => super::missing_key,
    /// A row with no id is named by nothing and fails at its header.
    missing_id: FailsWith(&[
        "budgets.toml:1: budget has no id\n    \
         fix: add id = \"snake_case_name\", unique in budgets.toml",
    ]), inspects 1 => super::missing_id,
    /// A key no row may have stops the check at its line: no row was looked at.
    unknown_key: FailsWith(&[
        "budgets.toml:10: budgets.toml is not valid: unknown field `colour`, expected one of `id`, \
         `metric`, `limit`, `unit`, `gate`, `measured_by`, `status`, `measured`, `measured_on`, `source`\n    \
         fix: fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
         of budgets.toml",
    ]), inspects 0 => super::unknown_key,
    /// A misspelt `[[budgets]]` table is an unknown top-level key, not an empty file.
    misspelt_table: FailsWith(&[
        "budgets.toml:1: budgets.toml is not valid: unknown field `budgets`, expected `budget`\n    \
         fix: fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
         of budgets.toml",
    ]), inspects 0 => super::misspelt_table,
    /// A key above the first table is a top-level key, so it is unknown too.
    key_before_first_table: FailsWith(&[
        "budgets.toml:1: budgets.toml is not valid: unknown field `limit`, expected `budget`\n    \
         fix: fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
         of budgets.toml",
    ]), inspects 0 => super::key_before_first_table,
    /// A string where the limit's number goes fails at its line.
    string_limit: FailsWith(&[
        "budgets.toml:4: budgets.toml is not valid: invalid type: string \"250\", expected f64\n    \
         fix: fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
         of budgets.toml",
    ]), inspects 0 => super::string_limit,
    /// A number where a string goes fails at its line.
    number_metric: FailsWith(&[
        "budgets.toml:3: budgets.toml is not valid: invalid type: integer `4`, expected a string\n    \
         fix: fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
         of budgets.toml",
    ]), inspects 0 => super::number_metric,
    /// A unit outside the set fails at its line.
    unit_outside_set: FailsWith(&[
        "budgets.toml:5: budgets.toml is not valid: unknown variant `seconds`, expected one of `s`, `ms`, `MB`, `KB`, `count`\n    \
         fix: fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
         of budgets.toml",
    ]), inspects 0 => super::unit_outside_set,
    /// A key given twice in one row fails at the second line, before either value is used.
    key_given_twice: FailsWith(&[
        "budgets.toml:10: budgets.toml is not valid: duplicate key\n    \
         fix: fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
         of budgets.toml",
    ]), inspects 0 => super::key_given_twice,
    /// An infinite limit is no limit: it fails like a negative one.
    infinite_limit: FailsWith(&[
        "budgets.toml:4: budget window_visible: limit must be zero or more\n    \
         fix: set limit to zero or more, in the row's unit",
    ]), inspects 1 => super::infinite_limit,
    /// A NaN limit compares false with everything, so it fails too.
    nan_limit: FailsWith(&[
        "budgets.toml:4: budget window_visible: limit must be zero or more\n    \
         fix: set limit to zero or more, in the row's unit",
    ]), inspects 1 => super::nan_limit,
    /// An id that is not snake_case fails at the id line.
    id_not_snake_case: FailsWith(&[
        "budgets.toml:2: budget Window-Visible: id must be snake_case\n    \
         fix: rename the id to lowercase words joined by _, such as window_visible",
    ]), inspects 1 => super::id_not_snake_case,
    /// A second row with the same id fails at its id line; both rows are read.
    duplicate_id: FailsWith(&[
        "budgets.toml:12: budget window_visible is defined twice (first at line 1)\n    \
         fix: give this row a new id, or remove one of the two tables",
    ]), inspects 2 => super::duplicate_id,
    /// A measured_on that is not lowercase words joined by - fails at its line.
    measured_on_shape: FailsWith(&[
        "budgets.toml:11: budget window_visible: measured_on must be lowercase words joined by -\n    \
         fix: write measured_on as lowercase words joined by -, such as macos-arm64",
    ]), inspects 1 => super::measured_on_shape,
    /// measured without measured_on fails at the header.
    measured_without_on: FailsWith(&[
        "budgets.toml:1: budget window_visible has measured but no measured_on\n    \
         fix: add measured_on = \"os-arch-machine kind, such as macos-arm64\"",
    ]), inspects 1 => super::measured_without_on,
    /// measured_on without measured fails at the header.
    on_without_measured: FailsWith(&[
        "budgets.toml:1: budget window_visible has measured_on but no measured\n    \
         fix: add measured = <number> in the row's unit, or remove measured_on",
    ]), inspects 1 => super::on_without_measured,
    /// Status measured with no measured value fails at the header.
    measured_without_value: FailsWith(&[
        "budgets.toml:1: budget window_visible is measured but has no measured value\n    \
         fix: add measured = <number> in the row's unit, or set status = \"unmeasured\"",
    ]), inspects 1 => super::measured_without_value,
    /// A file with no table fails on line 1, and reads no row, so the zero line fails too.
    no_tables: FailsWith(&[
        "budgets.toml:1: no [[budget]] tables\n    \
         fix: add a [[budget]] table for each budget the plan sets",
        "0 budget rows inspected in budgets.toml at the repo root\n    \
         fix: budgets looks for budget rows in budgets.toml at the repo root; restore them, or, if \
         they moved, update the consts xtask/src/checks/budgets.rs builds that place from",
    ]), inspects 0 => super::no_tables,
    /// A missing file fails on line 1, and reads no row (the OS words the error).
    missing_file: FailsWith(super::MISSING_FILE), inspects 0 => super::missing_file,
}

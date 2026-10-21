//! The planted cases of the budgets check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). Their builders are in `tests/repos.rs`.

use crate::budgets::Fix;
use crate::checks::planted::fails;

crate::checks::planted::cases! {
    /// A row whose limit is below zero fails at the limit's line.
    negative_limit: Fails[fails(
        "budgets.toml:4",
        "budget window_visible: limit must be zero or more",
        Fix::NegativeLimit,
    )], inspects 1 => super::repos::negative_limit,
    /// A file that is not TOML fails, and the check stopped on it: no row was looked at.
    not_toml: Fails[fails(
        "budgets.toml:1",
        "budgets.toml is not valid: unclosed array table, expected `]`",
        Fix::Syntax,
    )], inspects 0 => super::repos::not_toml,
    /// A file that is not UTF-8 is there but cannot be read: the check stopped on it.
    not_utf8: Fails[fails(
        "budgets.toml:1",
        "could not read `budgets.toml`: stream did not contain valid UTF-8",
        crate::checks::Fix::NotUtf8,
    )], inspects 0 => super::repos::not_utf8,
    /// A row with every required key passes.
    valid_row: Passes, inspects 1 => super::repos::valid_row,
    /// A row missing a required key fails at its header, naming the key.
    missing_key: Fails[fails(
        "budgets.toml:1",
        "budget window_visible has no source",
        Fix::AddSource,
    )], inspects 1 => super::repos::missing_key,
    /// A row with no id is named by nothing and fails at its header.
    missing_id: Fails[fails("budgets.toml:1", "budget has no id", Fix::AddId)],
        inspects 1 => super::repos::missing_id,
    /// A key no row may have stops the check at its line: no row was looked at.
    unknown_key: Fails[fails(
        "budgets.toml:10",
        "budgets.toml is not valid: unknown field `colour`, expected one of `id`, `metric`, \
         `limit`, `unit`, `gate`, `measured_by`, `status`, `measured`, `measured_on`, `source`",
        Fix::Syntax,
    )], inspects 0 => super::repos::unknown_key,
    /// A misspelt `[[budgets]]` table is an unknown top-level key, not an empty file.
    misspelt_table: Fails[fails(
        "budgets.toml:1",
        "budgets.toml is not valid: unknown field `budgets`, expected `budget`",
        Fix::Syntax,
    )], inspects 0 => super::repos::misspelt_table,
    /// A key above the first table is a top-level key, so it is unknown too.
    key_before_first_table: Fails[fails(
        "budgets.toml:1",
        "budgets.toml is not valid: unknown field `limit`, expected `budget`",
        Fix::Syntax,
    )], inspects 0 => super::repos::key_before_first_table,
    /// A string where the limit's number goes fails at its line.
    string_limit: Fails[fails(
        "budgets.toml:4",
        "budgets.toml is not valid: invalid type: string \"250\", expected f64",
        Fix::Syntax,
    )], inspects 0 => super::repos::string_limit,
    /// A number where a string goes fails at its line.
    number_metric: Fails[fails(
        "budgets.toml:3",
        "budgets.toml is not valid: invalid type: integer `4`, expected a string",
        Fix::Syntax,
    )], inspects 0 => super::repos::number_metric,
    /// A unit outside the set fails at its line.
    unit_outside_set: Fails[fails(
        "budgets.toml:5",
        "budgets.toml is not valid: unknown variant `seconds`, expected one of `s`, `ms`, `MB`, \
         `KB`, `count`",
        Fix::Syntax,
    )], inspects 0 => super::repos::unit_outside_set,
    /// A key given twice in one row fails at the second line, before either value is used.
    key_given_twice: Fails[fails(
        "budgets.toml:10",
        "budgets.toml is not valid: duplicate key",
        Fix::Syntax,
    )], inspects 0 => super::repos::key_given_twice,
    /// An infinite limit is no limit: it fails like a negative one.
    infinite_limit: Fails[fails(
        "budgets.toml:4",
        "budget window_visible: limit must be zero or more",
        Fix::NegativeLimit,
    )], inspects 1 => super::repos::infinite_limit,
    /// A NaN limit compares false with everything, so it fails too.
    nan_limit: Fails[fails(
        "budgets.toml:4",
        "budget window_visible: limit must be zero or more",
        Fix::NegativeLimit,
    )], inspects 1 => super::repos::nan_limit,
    /// An id that is not `snake_case` fails at the id line.
    id_not_snake_case: Fails[fails(
        "budgets.toml:2",
        "budget Window-Visible: id must be snake_case",
        Fix::IdNotSnakeCase,
    )], inspects 1 => super::repos::id_not_snake_case,
    /// A second row with the same id fails at its id line; both rows are read.
    duplicate_id: Fails[fails(
        "budgets.toml:12",
        "budget window_visible is defined twice (first at line 1)",
        Fix::DuplicateId,
    )], inspects 2 => super::repos::duplicate_id,
    /// A `measured_on` that is not lowercase words joined by - fails at its line.
    measured_on_shape: Fails[fails(
        "budgets.toml:11",
        "budget window_visible: measured_on must be lowercase words joined by -",
        Fix::MeasuredOnShape,
    )], inspects 1 => super::repos::measured_on_shape,
    /// `measured` without `measured_on` fails at the header.
    measured_without_on: Fails[fails(
        "budgets.toml:1",
        "budget window_visible has measured but no measured_on",
        Fix::AddMeasuredOn,
    )], inspects 1 => super::repos::measured_without_on,
    /// `measured_on` without `measured` fails at the header.
    on_without_measured: Fails[fails(
        "budgets.toml:1",
        "budget window_visible has measured_on but no measured",
        Fix::AddMeasuredOrRemoveOn,
    )], inspects 1 => super::repos::on_without_measured,
    /// Status measured with no measured value fails at the header.
    measured_without_value: Fails[fails(
        "budgets.toml:1",
        "budget window_visible is measured but has no measured value",
        Fix::AddMeasuredOrUnmeasure,
    )], inspects 1 => super::repos::measured_without_value,
    /// A file with no table fails on line 1, and reads no row, so the zero line fails too.
    no_tables: Fails[
        fails("budgets.toml:1", "no [[budget]] tables", Fix::NoRows),
        super::nothing_inspected(),
    ], inspects 0 => super::repos::no_tables,
    /// A missing file fails on line 1, and reads no row (the OS words the error).
    missing_file: Fails(super::missing_file), inspects 0 => super::repos::missing_file,
}

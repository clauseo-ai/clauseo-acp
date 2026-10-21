//! The fix lines of the budgets check, one variant per failure kind. Each text is written once,
//! in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line for a problem in `budgets.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// The file is not there.
    Restore,
    /// `toml` cannot read the file.
    Syntax,
    /// The file has no `[[budget]]` table.
    NoRows,
    /// A limit below zero, or not a finite number.
    NegativeLimit,
    /// `measured_on` is not lowercase words joined by `-`.
    MeasuredOnShape,
    /// The id is not `snake_case`.
    IdNotSnakeCase,
    /// Two rows share an id.
    DuplicateId,
    /// The row has no `id`.
    AddId,
    AddMetric,
    AddLimit,
    AddUnit,
    AddGate,
    AddMeasuredBy,
    AddStatus,
    AddSource,
    /// `measured` without `measured_on`.
    AddMeasuredOn,
    /// `measured_on` without `measured`.
    AddMeasuredOrRemoveOn,
    /// Status `measured` with no `measured` value.
    AddMeasuredOrUnmeasure,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Fix::Restore => "restore budgets.toml at the repo root",
            Fix::Syntax => {
                "fix the line above; each row is a [[budget]] table of the ten keys listed at the \
                 top of budgets.toml"
            }
            Fix::NoRows => "add a [[budget]] table for each budget the plan sets",
            Fix::NegativeLimit => "set limit to zero or more, in the row's unit",
            Fix::MeasuredOnShape => {
                "write measured_on as lowercase words joined by -, such as macos-arm64"
            }
            Fix::IdNotSnakeCase => {
                "rename the id to lowercase words joined by _, such as window_visible"
            }
            Fix::DuplicateId => "give this row a new id, or remove one of the two tables",
            Fix::AddId => r#"add id = "snake_case_name", unique in budgets.toml"#,
            Fix::AddMetric => r#"add metric = "what is measured""#,
            Fix::AddLimit => "add limit = <number> in the row's unit",
            Fix::AddUnit => r#"add unit = "s", "ms", "MB", "KB" or "count""#,
            Fix::AddGate => r#"add gate = "pr", "release", "alert" or "local""#,
            Fix::AddMeasuredBy => r#"add measured_by = "the tool or method""#,
            Fix::AddStatus => r#"add status = "unmeasured" or "measured""#,
            Fix::AddSource => r#"add source = "the plan section it comes from""#,
            Fix::AddMeasuredOn => {
                r#"add measured_on = "os-arch-machine kind, such as macos-arm64""#
            }
            Fix::AddMeasuredOrRemoveOn => {
                "add measured = <number> in the row's unit, or remove measured_on"
            }
            Fix::AddMeasuredOrUnmeasure => {
                r#"add measured = <number> in the row's unit, or set status = "unmeasured""#
            }
        })
    }
}

#[cfg(test)]
mod tests;

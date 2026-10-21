//! Pins the text of every `budgets::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::Restore => "restore budgets.toml at the repo root",
        Fix::Syntax => {
            "fix the line above; each row is a [[budget]] table of the ten keys listed at the top \
             of budgets.toml"
        },
        Fix::NoRows => "add a [[budget]] table for each budget the plan sets",
        Fix::NegativeLimit => "set limit to zero or more, in the row's unit",
        Fix::MeasuredOnShape => {
            "write measured_on as lowercase words joined by -, such as macos-arm64"
        },
        Fix::IdNotSnakeCase => {
            "rename the id to lowercase words joined by _, such as window_visible"
        },
        Fix::DuplicateId => "give this row a new id, or remove one of the two tables",
        Fix::AddId => r#"add id = "snake_case_name", unique in budgets.toml"#,
        Fix::AddMetric => r#"add metric = "what is measured""#,
        Fix::AddLimit => "add limit = <number> in the row's unit",
        Fix::AddUnit => r#"add unit = "s", "ms", "MB", "KB" or "count""#,
        Fix::AddGate => r#"add gate = "pr", "release", "alert" or "local""#,
        Fix::AddMeasuredBy => r#"add measured_by = "the tool or method""#,
        Fix::AddStatus => r#"add status = "unmeasured" or "measured""#,
        Fix::AddSource => r#"add source = "the plan section it comes from""#,
        Fix::AddMeasuredOn => r#"add measured_on = "os-arch-machine kind, such as macos-arm64""#,
        Fix::AddMeasuredOrRemoveOn => {
            "add measured = <number> in the row's unit, or remove measured_on"
        },
        Fix::AddMeasuredOrUnmeasure => {
            r#"add measured = <number> in the row's unit, or set status = "unmeasured""#
        },
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [
        Fix::Restore,
        Fix::Syntax,
        Fix::NoRows,
        Fix::NegativeLimit,
        Fix::MeasuredOnShape,
        Fix::IdNotSnakeCase,
        Fix::DuplicateId,
        Fix::AddId,
        Fix::AddMetric,
        Fix::AddLimit,
        Fix::AddUnit,
        Fix::AddGate,
        Fix::AddMeasuredBy,
        Fix::AddStatus,
        Fix::AddSource,
        Fix::AddMeasuredOn,
        Fix::AddMeasuredOrRemoveOn,
        Fix::AddMeasuredOrUnmeasure,
    ];
    assert_texts(&samples, pinned, ARMS);
}

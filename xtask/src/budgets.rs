//! `budgets.toml`: one `[[budget]]` table per metric the plan budgets.
//!
//! The `toml` crate reads the file into `File` and `Row`. A `Row` names every key a row may have,
//! each with its type and its enum values, and `deny_unknown_fields` rejects any other key. Each
//! value keeps its span, so a problem is reported at the line that causes it. The rules that `toml`
//! cannot express stay here: required keys, `snake_case` and unique ids, `measured` and `measured_on`
//! set together, and a limit that is zero or more.

use crate::violation::Violation;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use toml::Spanned;

/// The file's name, at the repo root.
pub(crate) const FILE: &str = "budgets.toml";

/// One budget row that passed every rule. Only the fields `buildtime` reads are kept; the other
/// keys are checked when the file is read and then dropped.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Budget {
    pub id: String,
    pub limit: f64,
    pub unit: Unit,
    pub gate: Gate,
}

/// The unit a limit is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum Unit {
    #[serde(rename = "s")]
    S,
    #[serde(rename = "ms")]
    Ms,
    #[serde(rename = "MB")]
    Mb,
    #[serde(rename = "KB")]
    Kb,
    #[serde(rename = "count")]
    Count,
}

impl Unit {
    /// The unit as `budgets.toml` writes it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Unit::S => "s",
            Unit::Ms => "ms",
            Unit::Mb => "MB",
            Unit::Kb => "KB",
            Unit::Count => "count",
        }
    }
}

/// Which gate a budget is enforced by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Gate {
    Pr,
    Release,
    Alert,
    Local,
}

/// Whether the row's `measured` value came from this repo's own tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Unmeasured,
    Measured,
}

/// The file as `toml` reads it. `budget` is empty when there is no table, which is a rule of its own.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    budget: Vec<Spanned<Row>>,
}

/// One `[[budget]]` table. Its fields are the only keys a row may have; a key it lacks is `None`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: Option<Spanned<String>>,
    metric: Option<Spanned<String>>,
    limit: Option<Spanned<f64>>,
    unit: Option<Spanned<Unit>>,
    gate: Option<Spanned<Gate>>,
    measured_by: Option<Spanned<String>>,
    status: Option<Spanned<Status>>,
    measured: Option<Spanned<f64>>,
    measured_on: Option<Spanned<String>>,
    source: Option<Spanned<String>>,
}

/// Reads `text` as `budgets.toml`. Returns the rows without problems, in file order, and every
/// problem found, sorted by line.
#[cfg(test)]
pub(crate) fn parse(text: &str) -> (Vec<Budget>, Vec<Violation>) {
    let (_, budgets, violations) = parse_counted(text);
    (budgets, violations)
}

/// `parse`, plus the number of `[[budget]]` rows read, with or without problems. A file `toml`
/// cannot read has none.
fn parse_counted(text: &str) -> (usize, Vec<Budget>, Vec<Violation>) {
    let file: File = match toml::from_str(text) {
        Ok(file) => file,
        Err(error) => return (0, Vec::new(), vec![syntax_problem(text, &error)]),
    };
    let rows = file.budget.len();
    let mut violations = Vec::new();
    if file.budget.is_empty() {
        violations.push(problem(
            1,
            "no [[budget]] tables",
            "add a [[budget]] table for each budget the plan sets",
        ));
    }
    let mut budgets = Vec::new();
    let mut first_headers = HashMap::new();
    for row in file.budget {
        let header = line_number(text, row.span().start);
        let row = row.into_inner();
        if check_row(&row, text, header, &mut first_headers, &mut violations) {
            budgets.extend(into_budget(row));
        }
    }
    violations.sort_by_key(|violation| violation.line);
    (rows, budgets, violations)
}

/// Reads `budgets.toml` under `root`. `Err` holds every problem, including an unreadable file.
pub(crate) fn load(root: &Path) -> Result<Vec<Budget>, Vec<Violation>> {
    let (_, budgets, violations) = read(root);
    if violations.is_empty() {
        Ok(budgets)
    } else {
        Err(violations)
    }
}

/// Reads `budgets.toml` under `root`: the number of rows read, with or without problems, and
/// every problem, including an unreadable file (which has no rows).
pub(crate) fn inspect(root: &Path) -> (usize, Vec<Violation>) {
    let (rows, _, violations) = read(root);
    (rows, violations)
}

/// `parse_counted` on `budgets.toml` under `root`; an unreadable file is one problem, no rows.
fn read(root: &Path) -> (usize, Vec<Budget>, Vec<Violation>) {
    match std::fs::read_to_string(root.join(FILE)) {
        Ok(text) => parse_counted(&text),
        Err(error) => (
            0,
            Vec::new(),
            vec![problem(
                1,
                format!("cannot read budgets.toml: {error}"),
                "restore budgets.toml at the repo root",
            )],
        ),
    }
}

fn problem(line: u32, message: impl Into<String>, fix: impl Into<String>) -> Violation {
    Violation {
        path: FILE.to_owned(),
        line,
        message: message.into(),
        fix: fix.into(),
    }
}

/// A file `toml` cannot read: one violation, at the line of the error.
fn syntax_problem(text: &str, error: &toml::de::Error) -> Violation {
    let line = error.span().map_or(1, |span| line_number(text, span.start));
    let first = error.message().lines().next().unwrap_or_default().trim();
    problem(
        line,
        format!("budgets.toml is not valid: {first}"),
        "fix the line above; each row is a [[budget]] table of the ten keys listed at the top of budgets.toml",
    )
}

/// The 1-based line that byte `offset` of `text` is on.
fn line_number(text: &str, offset: usize) -> u32 {
    let head = text.get(..offset).unwrap_or(text);
    u32::try_from(head.matches('\n').count() + 1).unwrap_or(u32::MAX)
}

/// The line a value starts on.
fn line_of<T>(value: &Spanned<T>, text: &str) -> u32 {
    line_number(text, value.span().start)
}

/// `^[a-z][a-z0-9]*(_[a-z0-9]+)*$`.
fn is_snake_case(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_lowercase())
        && text.split('_').all(|part| lower_or_digits(part, &[]))
}

/// `^[a-z0-9_]+(-[a-z0-9_]+)*$`.
fn is_measured_on(text: &str) -> bool {
    text.split('-').all(|part| lower_or_digits(part, b"_"))
}

/// Whether `part` is one or more lowercase letters or digits, or any byte in `extra`.
fn lower_or_digits(part: &str, extra: &[u8]) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || extra.contains(&b))
}

/// The name a row's messages use: its id, or its header line when the id is missing.
fn row_name(row: &Row, header: u32) -> String {
    row.id
        .as_ref()
        .map_or_else(|| format!("at line {header}"), |id| id.get_ref().clone())
}

/// Checks one row, and returns whether it has no problem. Problems go to `found`.
fn check_row(
    row: &Row,
    text: &str,
    header: u32,
    first_headers: &mut HashMap<String, u32>,
    found: &mut Vec<Violation>,
) -> bool {
    let before = found.len();
    let name = row_name(row, header);
    if let Some(id) = &row.id {
        check_id(id, text, header, first_headers, found);
    }
    if let Some(limit) = &row.limit
        && !(limit.get_ref().is_finite() && *limit.get_ref() >= 0.0)
    {
        found.push(problem(
            line_of(limit, text),
            format!("budget {name}: limit must be zero or more"),
            "set limit to zero or more, in the row's unit",
        ));
    }
    if let Some(measured_on) = &row.measured_on
        && !is_measured_on(measured_on.get_ref())
    {
        found.push(problem(
            line_of(measured_on, text),
            format!("budget {name}: measured_on must be lowercase words joined by -"),
            "write measured_on as lowercase words joined by -, such as macos-arm64",
        ));
    }
    check_required(row, &name, header, found);
    check_measured(row, &name, header, found);
    found.len() == before
}

/// The id is `snake_case`, and no earlier row has it. `first_headers` maps each id to the
/// header line of the row that first had it.
fn check_id(
    id: &Spanned<String>,
    text: &str,
    header: u32,
    first_headers: &mut HashMap<String, u32>,
    found: &mut Vec<Violation>,
) {
    let line = line_of(id, text);
    let value = id.get_ref();
    if !is_snake_case(value) {
        found.push(problem(
            line,
            format!("budget {value}: id must be snake_case"),
            "rename the id to lowercase words joined by _, such as window_visible",
        ));
    }
    match first_headers.get(value) {
        Some(first) => found.push(problem(
            line,
            format!("budget {value} is defined twice (first at line {first})"),
            "give this row a new id, or remove one of the two tables",
        )),
        None => {
            first_headers.insert(value.clone(), header);
        }
    }
}

/// Every required key is there. A row that lacks one gets the fix for that key.
fn check_required(row: &Row, name: &str, header: u32, found: &mut Vec<Violation>) {
    #[rustfmt::skip]
    let required = [
        (row.id.is_none(), "id", r#"add id = "snake_case_name", unique in budgets.toml"#),
        (row.metric.is_none(), "metric", r#"add metric = "what is measured""#),
        (row.limit.is_none(), "limit", "add limit = <number> in the row's unit"),
        (row.unit.is_none(), "unit", r#"add unit = "s", "ms", "MB", "KB" or "count""#),
        (row.gate.is_none(), "gate", r#"add gate = "pr", "release", "alert" or "local""#),
        (row.measured_by.is_none(), "measured_by", r#"add measured_by = "the tool or method""#),
        (row.status.is_none(), "status", r#"add status = "unmeasured" or "measured""#),
        (row.source.is_none(), "source", r#"add source = "the plan section it comes from""#),
    ];
    for (missing, key, fix) in required {
        if missing {
            let message = if key == "id" {
                "budget has no id".to_owned()
            } else {
                format!("budget {name} has no {key}")
            };
            found.push(problem(header, message, fix));
        }
    }
}

/// `measured` and `measured_on` are set together, and a row with status `measured` has a value.
fn check_measured(row: &Row, name: &str, header: u32, found: &mut Vec<Violation>) {
    let has_measured = row.measured.is_some();
    let has_measured_on = row.measured_on.is_some();
    if has_measured && !has_measured_on {
        found.push(problem(
            header,
            format!("budget {name} has measured but no measured_on"),
            r#"add measured_on = "os-arch-machine kind, such as macos-arm64""#,
        ));
    }
    if has_measured_on && !has_measured {
        found.push(problem(
            header,
            format!("budget {name} has measured_on but no measured"),
            "add measured = <number> in the row's unit, or remove measured_on",
        ));
    }
    let is_measured = row
        .status
        .as_ref()
        .is_some_and(|status| *status.get_ref() == Status::Measured);
    if is_measured && !has_measured {
        found.push(problem(
            header,
            format!("budget {name} is measured but has no measured value"),
            r#"add measured = <number> in the row's unit, or set status = "unmeasured""#,
        ));
    }
}

/// The row as a `Budget`. `None` when a required key is missing, which `check_row` reports first.
fn into_budget(row: Row) -> Option<Budget> {
    Some(Budget {
        id: row.id?.into_inner(),
        limit: row.limit?.into_inner(),
        unit: row.unit?.into_inner(),
        gate: row.gate?.into_inner(),
    })
}

#[cfg(test)]
mod tests;

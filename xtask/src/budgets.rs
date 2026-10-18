//! `budgets.toml`: one `[[budget]]` table per metric the plan budgets.
//!
//! This is a line reader, not a TOML parser. It reads only the shape the file uses: `[[budget]]`
//! headers, and `key = "text"` or `key = number` lines, each with an optional `# comment`. Every
//! problem is a `Violation` at the line that causes it, and a row with any problem is not returned.

mod lex;

use crate::violation::Violation;
use lex::{Line, Value, read_line};
use std::collections::HashMap;
use std::path::Path;

/// The file's name, at the repo root.
pub(crate) const FILE: &str = "budgets.toml";

/// One budget row that passed every rule.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Budget {
    pub id: String,
    pub metric: String,
    pub limit: f64,
    pub unit: Unit,
    pub gate: Gate,
    pub measured_by: String,
    pub status: Status,
    pub measured: Option<f64>,
    pub measured_on: Option<String>,
    pub source: String,
    /// The line of the row's `[[budget]]` header.
    pub line: u32,
}

/// The unit a limit is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unit {
    S,
    Ms,
    Mb,
    Kb,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    Pr,
    Release,
    Alert,
    Local,
}

/// Whether the row's `measured` value came from this repo's own tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Unmeasured,
    Measured,
}

const UNITS: &[(&str, Unit)] = &[
    ("s", Unit::S),
    ("ms", Unit::Ms),
    ("MB", Unit::Mb),
    ("KB", Unit::Kb),
    ("count", Unit::Count),
];
const GATES: &[(&str, Gate)] = &[
    ("pr", Gate::Pr),
    ("release", Gate::Release),
    ("alert", Gate::Alert),
    ("local", Gate::Local),
];
const STATUSES: &[(&str, Status)] = &[
    ("unmeasured", Status::Unmeasured),
    ("measured", Status::Measured),
];

/// The type a key's value must have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Number,
}

/// The ten keys a row may have, with the type each takes.
const KEYS: &[(&str, Kind)] = &[
    ("id", Kind::Text),
    ("metric", Kind::Text),
    ("limit", Kind::Number),
    ("unit", Kind::Text),
    ("gate", Kind::Text),
    ("measured_by", Kind::Text),
    ("status", Kind::Text),
    ("measured", Kind::Number),
    ("measured_on", Kind::Text),
    ("source", Kind::Text),
];

/// The keys every row needs, with the fix for a row that lacks one.
const REQUIRED: &[(&str, &str)] = &[
    (
        "id",
        r#"add id = "snake_case_name", unique in budgets.toml"#,
    ),
    ("metric", r#"add metric = "what is measured""#),
    ("limit", "add limit = <number> in the row's unit"),
    ("unit", r#"add unit = "s", "ms", "MB", "KB" or "count""#),
    ("gate", r#"add gate = "pr", "release", "alert" or "local""#),
    ("measured_by", r#"add measured_by = "the tool or method""#),
    ("status", r#"add status = "unmeasured" or "measured""#),
    ("source", r#"add source = "the plan section it comes from""#),
];

const FORMAT_FIX: &str =
    r#"budgets.toml holds only [[budget]] headers and key = "text" or key = number lines"#;

const UNKNOWN_KEY_FIX: &str = "use one of the ten keys: id, metric, limit, unit, gate, measured_by, status, measured, measured_on, source";

/// One `key = value` line, with its line number.
struct Pair<'a> {
    key: &'a str,
    value: Value,
    line: u32,
}

/// One `[[budget]]` table: its header line, its pairs, and whether a line in it was unreadable.
struct Table<'a> {
    header: u32,
    pairs: Vec<Pair<'a>>,
    broken: bool,
}

/// What a row's keys hold. A key that is present but wrong stays unset, and a violation says so.
#[derive(Default)]
struct Values {
    id: Option<String>,
    metric: Option<String>,
    limit: Option<f64>,
    unit: Option<Unit>,
    gate: Option<Gate>,
    measured_by: Option<String>,
    status: Option<Status>,
    measured: Option<f64>,
    measured_on: Option<String>,
    source: Option<String>,
}

/// Reads `text` as `budgets.toml`. Returns the rows without problems, in file order, and every
/// problem found, sorted by line.
pub(crate) fn parse(text: &str) -> (Vec<Budget>, Vec<Violation>) {
    let mut violations = Vec::new();
    let mut tables: Vec<Table<'_>> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        match read_line(raw) {
            Line::Blank => {}
            Line::Header => tables.push(Table {
                header: line,
                pairs: Vec::new(),
                broken: false,
            }),
            Line::Pair { key, value } => match tables.last_mut() {
                Some(table) => table.pairs.push(Pair { key, value, line }),
                None => violations.push(problem(
                    line,
                    format!("key {key} is outside a [[budget]] table"),
                    "move it under a [[budget]] header",
                )),
            },
            Line::Unreadable => {
                violations.push(problem(line, "cannot read this line", FORMAT_FIX));
                if let Some(table) = tables.last_mut() {
                    table.broken = true;
                }
            }
        }
    }
    if tables.is_empty() {
        violations.push(problem(
            1,
            "no [[budget]] tables",
            "add a [[budget]] table for each budget the plan sets",
        ));
    }

    let mut budgets = Vec::new();
    let mut first_lines = HashMap::new();
    for table in &tables {
        let (found, budget) = check_table(table, &mut first_lines);
        violations.extend(found);
        budgets.extend(budget);
    }
    violations.sort_by_key(|violation| violation.line);
    (budgets, violations)
}

/// Reads `budgets.toml` under `root`. `Err` holds every problem, including an unreadable file.
pub(crate) fn load(root: &Path) -> Result<Vec<Budget>, Vec<Violation>> {
    let text = std::fs::read_to_string(root.join(FILE)).map_err(|error| {
        vec![problem(
            1,
            format!("cannot read budgets.toml: {error}"),
            "restore budgets.toml at the repo root",
        )]
    })?;
    let (budgets, violations) = parse(&text);
    if violations.is_empty() {
        Ok(budgets)
    } else {
        Err(violations)
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

/// `^[a-z][a-z0-9]*(_[a-z0-9]+)*$`.
fn is_snake_case(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_lowercase())
        && text.split('_').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// `^[a-z0-9_]+(-[a-z0-9_]+)*$`.
fn is_measured_on(text: &str) -> bool {
    text.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    })
}

/// The name a row's messages use: its id, or its line when the id is missing or not text.
fn row_name(table: &Table<'_>) -> String {
    let id = table
        .pairs
        .iter()
        .find(|pair| pair.key == "id")
        .and_then(|pair| pair.value.text());
    id.map_or_else(|| format!("at line {}", table.header), str::to_owned)
}

/// Checks one table. Returns its problems, and its budget when it has none.
fn check_table(
    table: &Table<'_>,
    first_lines: &mut HashMap<String, u32>,
) -> (Vec<Violation>, Option<Budget>) {
    let name = row_name(table);
    let mut found = Vec::new();
    let mut values = Values::default();
    let mut seen: Vec<&str> = Vec::new();
    for pair in &table.pairs {
        if seen.contains(&pair.key) {
            found.push(problem(
                pair.line,
                format!("budget {name}: {} appears twice", pair.key),
                format!("keep one {} line in this table", pair.key),
            ));
            continue;
        }
        seen.push(pair.key);
        read_pair(
            pair,
            &name,
            table.header,
            first_lines,
            &mut values,
            &mut found,
        );
    }
    check_row(table, &name, &seen, &values, &mut found);

    let budget = if found.is_empty() && !table.broken {
        into_budget(values, table.header)
    } else {
        None
    };
    (found, budget)
}

/// Checks one key's type and value, and stores the value.
fn read_pair(
    pair: &Pair<'_>,
    name: &str,
    header: u32,
    first_lines: &mut HashMap<String, u32>,
    values: &mut Values,
    found: &mut Vec<Violation>,
) {
    let line = pair.line;
    let Some(kind) = KEYS
        .iter()
        .find(|(key, _)| *key == pair.key)
        .map(|(_, kind)| *kind)
    else {
        found.push(problem(
            line,
            format!("budget {name}: unknown key {}", pair.key),
            UNKNOWN_KEY_FIX,
        ));
        return;
    };
    match (kind, &pair.value) {
        (Kind::Text, Value::Number(_)) => found.push(problem(
            line,
            format!("budget {name}: {} must be a string", pair.key),
            format!("write {} as a quoted string", pair.key),
        )),
        (Kind::Number, Value::Text(_)) => found.push(problem(
            line,
            format!("budget {name}: {} must be a number", pair.key),
            format!("write {} as a number, without quotes", pair.key),
        )),
        (Kind::Text, Value::Text(text)) => {
            read_text(
                pair.key,
                text,
                line,
                name,
                header,
                first_lines,
                values,
                found,
            );
        }
        (Kind::Number, Value::Number(number)) => {
            read_number(pair.key, *number, line, name, values, found);
        }
    }
}

fn read_text(
    key: &str,
    text: &str,
    line: u32,
    name: &str,
    header: u32,
    first_lines: &mut HashMap<String, u32>,
    values: &mut Values,
    found: &mut Vec<Violation>,
) {
    match key {
        "id" => {
            if !is_snake_case(text) {
                found.push(problem(
                    line,
                    format!("budget {name}: id must be snake_case"),
                    "rename the id to lowercase words joined by _, such as window_visible",
                ));
            }
            match first_lines.get(text) {
                Some(first) => found.push(problem(
                    line,
                    format!("budget {text} is defined twice (first at line {first})"),
                    "give this row a new id, or remove one of the two tables",
                )),
                None => {
                    first_lines.insert(text.to_owned(), header);
                }
            }
            values.id = Some(text.to_owned());
        }
        "metric" => values.metric = Some(text.to_owned()),
        "measured_by" => values.measured_by = Some(text.to_owned()),
        "source" => values.source = Some(text.to_owned()),
        "measured_on" => {
            if !is_measured_on(text) {
                found.push(problem(
                    line,
                    format!("budget {name}: measured_on must be lowercase words joined by -"),
                    "write measured_on as lowercase words joined by -, such as macos-arm64",
                ));
            }
            values.measured_on = Some(text.to_owned());
        }
        "unit" => values.unit = read_choice(UNITS, key, text, line, name, found),
        "gate" => values.gate = read_choice(GATES, key, text, line, name, found),
        "status" => values.status = read_choice(STATUSES, key, text, line, name, found),
        _ => {}
    }
}

/// Looks `text` up in `choices`, and reports it when it is not there.
fn read_choice<T: Copy>(
    choices: &[(&str, T)],
    key: &str,
    text: &str,
    line: u32,
    name: &str,
    found: &mut Vec<Violation>,
) -> Option<T> {
    let value = choices
        .iter()
        .find(|(choice, _)| *choice == text)
        .map(|(_, value)| *value);
    if value.is_none() {
        let names: Vec<&str> = choices.iter().map(|(choice, _)| *choice).collect();
        let quoted: Vec<String> = names.iter().map(|choice| format!("\"{choice}\"")).collect();
        found.push(problem(
            line,
            format!(
                "budget {name}: {key} \"{text}\" is not one of {}",
                names.join(", ")
            ),
            format!("set {key} to one of {}", quoted.join(", ")),
        ));
    }
    value
}

fn read_number(
    key: &str,
    number: f64,
    line: u32,
    name: &str,
    values: &mut Values,
    found: &mut Vec<Violation>,
) {
    match key {
        "limit" => {
            if !(number.is_finite() && number >= 0.0) {
                found.push(problem(
                    line,
                    format!("budget {name}: limit must be zero or more"),
                    "set limit to zero or more, in the row's unit",
                ));
            }
            values.limit = Some(number);
        }
        "measured" => values.measured = Some(number),
        _ => {}
    }
}

/// The row-level rules: every required key is there, and `measured` and `measured_on` agree.
fn check_row(
    table: &Table<'_>,
    name: &str,
    seen: &[&str],
    values: &Values,
    found: &mut Vec<Violation>,
) {
    let header = table.header;
    for (key, fix) in REQUIRED {
        if seen.contains(key) {
            continue;
        }
        let message = if *key == "id" {
            "budget has no id".to_owned()
        } else {
            format!("budget {name} has no {key}")
        };
        found.push(problem(header, message, *fix));
    }
    let has_measured = seen.contains(&"measured");
    let has_measured_on = seen.contains(&"measured_on");
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
    if values.status == Some(Status::Measured) && !has_measured {
        found.push(problem(
            header,
            format!("budget {name} is measured but has no measured value"),
            r#"add measured = <number> in the row's unit, or set status = "unmeasured""#,
        ));
    }
}

/// The row as a `Budget`. `None` when a required key is missing or wrong.
fn into_budget(values: Values, line: u32) -> Option<Budget> {
    Some(Budget {
        id: values.id?,
        metric: values.metric?,
        limit: values.limit?,
        unit: values.unit?,
        gate: values.gate?,
        measured_by: values.measured_by?,
        status: values.status?,
        measured: values.measured,
        measured_on: values.measured_on,
        source: values.source?,
        line,
    })
}

#[cfg(test)]
mod tests;

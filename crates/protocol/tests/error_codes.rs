//! `docs/decisions/0003-error-codes.md` is the record of the error codes. This test keeps the table
//! in step with `ErrorCode`: every code has a row, every row names a known code, and the
//! retryable column matches `ErrorCode::is_retryable`.

use std::collections::BTreeSet;

use clauseo_protocol::ErrorCode;

/// The record, read at build time.
const RECORD: &str = include_str!("../../../docs/decisions/0003-error-codes.md");

/// One table row: its code, and whether its retryable cell says `yes`.
struct Row {
    code: String,
    retryable: Option<bool>,
}

/// The rows whose first cell is a backticked code. Other tables and prose are skipped.
fn rows(record: &str) -> Vec<Row> {
    record
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let code = cells.get(1)?.strip_prefix('`')?.strip_suffix('`')?;
            let retryable = match *cells.get(3)? {
                "yes" => Some(true),
                "no" => Some(false),
                _ => None,
            };
            Some(Row {
                code: code.to_owned(),
                retryable,
            })
        })
        .collect()
}

#[test]
fn error_codes_match_record() {
    let rows = rows(RECORD);
    let mut problems = Vec::new();

    let mut seen = BTreeSet::new();
    for row in &rows {
        if !seen.insert(row.code.as_str()) {
            problems.push(format!("`{}` has more than one row", row.code));
        }
    }

    for code in ErrorCode::ALL {
        let name = code.as_str();
        match rows.iter().find(|row| row.code == name) {
            None => problems.push(format!("`{name}` has no row")),
            Some(row) => {
                if row.retryable != Some(code.is_retryable()) {
                    problems.push(format!(
                        "`{name}` is retryable: {} in the code, but its row says {}",
                        code.is_retryable(),
                        match row.retryable {
                            Some(true) => "yes",
                            Some(false) => "no",
                            None => "neither yes nor no",
                        }
                    ));
                }
            }
        }
    }

    for row in &rows {
        if !ErrorCode::ALL.iter().any(|code| code.as_str() == row.code) {
            problems.push(format!("the row `{}` names no ErrorCode", row.code));
        }
    }

    assert!(
        problems.is_empty(),
        "docs/decisions/0003-error-codes.md disagrees with ErrorCode:\n{}",
        problems.join("\n")
    );
}

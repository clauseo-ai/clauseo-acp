//! `docs/decisions/0003-error-codes.md` is the record of the error codes. This test keeps the table
//! in step with `ErrorCode`: every code has a row, every row names a known code, the meaning
//! column is `ErrorCode::meaning`, and the retryable column matches `ErrorCode::is_retryable`.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/markdown.rs"]
pub mod markdown;

use std::collections::BTreeSet;

use clauseo_protocol::ErrorCode;

/// The record, read at build time.
const RECORD: &str = include_str!("../../../docs/decisions/0003-error-codes.md");

/// One table row: its code, its meaning cell, and whether its retryable cell says `yes`.
struct Row {
    code: String,
    meaning: String,
    retryable: Option<bool>,
}

/// The rows of the code table under `## Decision`, as a reader sees it (`support/markdown.rs`):
/// a row in a code block is no row. A code cell that is not one code span is read as written, so
/// it names no code and fails the test.
fn rows(record: &str) -> Vec<Row> {
    let table = markdown::table(record, "Decision", "code");
    let codes = table.cells("code");
    let meanings = table.cells("meaning");
    let retryables = table.cells("retryable");
    codes
        .into_iter()
        .zip(meanings)
        .zip(retryables)
        .map(|((code, meaning), retryable)| Row {
            code: match code.code.as_slice() {
                [one] if code.text == format!("`{one}`") => one.clone(),
                _ => code.text.clone(),
            },
            meaning: meaning.text.clone(),
            retryable: match retryable.text.as_str() {
                "yes" => Some(true),
                "no" => Some(false),
                _ => None,
            },
        })
        .collect()
}

#[test]
fn unknown_is_the_last_code() {
    assert_eq!(ErrorCode::ALL.last(), Some(&ErrorCode::Unknown));
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
                if row.meaning != code.meaning() {
                    problems.push(format!(
                        "`{name}` means {:?} in the code, but its row says {:?}",
                        code.meaning(),
                        row.meaning
                    ));
                }
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

#[test]
fn display_writes_the_code_text() {
    assert_eq!(
        ErrorCode::StoreNewerVersion.to_string(),
        "store.newer_version"
    );
    assert_eq!(ErrorCode::Unknown.to_string(), "unknown");
}

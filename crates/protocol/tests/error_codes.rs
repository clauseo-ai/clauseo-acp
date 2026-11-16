//! `docs/decisions/0003-error-codes.md` is the record of the error codes. This test keeps the table
//! in step with `ErrorCode`: every code has a row, every row names a known code, the meaning
//! column is `ErrorCode::meaning`, and the retryable column matches `ErrorCode::is_retryable`.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/markdown.rs"]
pub mod markdown;

use std::collections::BTreeSet;

use clauseo_protocol::{
    ErrorCode, FileRefusal, HostError, MAX_ERROR_FILES, MatterPath, RefusedBecause, WriteFailure,
};
use serde_json::json;

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

/// `matter.folder_not_a_matter`: its string, not retryable, and its row in the record, which says
/// so too.
#[test]
fn folder_not_a_matter_is_its_own_code_and_not_retryable() {
    let code = ErrorCode::MatterFolderNotAMatter;
    assert_eq!(code.as_str(), "matter.folder_not_a_matter");
    assert_eq!(
        "matter.folder_not_a_matter".parse::<ErrorCode>().ok(),
        Some(code)
    );
    assert!(!code.is_retryable());
    assert!(!HostError::new(code, "no marker").retryable());
    let row = rows(RECORD)
        .into_iter()
        .find(|row| row.code == "matter.folder_not_a_matter")
        .expect("0003 has a row for matter.folder_not_a_matter");
    assert_eq!(row.retryable, Some(false));
    assert_eq!(row.meaning, code.meaning());
    let index = |wanted| ErrorCode::ALL.iter().position(|code| *code == wanted);
    assert!(index(code) < index(ErrorCode::Unknown));
}

/// `matter.folder_permission`: its string, not retryable (the lawyer changes a privacy setting
/// first), its row in the record, and its place before `unknown`.
#[test]
fn folder_permission_is_its_own_code_and_not_retryable() {
    let code = ErrorCode::MatterFolderPermission;
    assert_eq!(code.as_str(), "matter.folder_permission");
    assert_eq!(
        "matter.folder_permission".parse::<ErrorCode>().ok(),
        Some(code)
    );
    assert!(!code.is_retryable());
    assert!(!HostError::new(code, "no access").retryable());
    let row = rows(RECORD)
        .into_iter()
        .find(|row| row.code == "matter.folder_permission")
        .expect("0003 has a row for matter.folder_permission");
    assert_eq!(row.retryable, Some(false));
    assert_eq!(row.meaning, code.meaning());
    let index = |wanted| ErrorCode::ALL.iter().position(|code| *code == wanted);
    assert!(index(code) < index(ErrorCode::Unknown));
}

#[test]
fn display_writes_the_code_text() {
    assert_eq!(
        ErrorCode::StoreNewerVersion.to_string(),
        "store.newer_version"
    );
    assert_eq!(ErrorCode::Unknown.to_string(), "unknown");
}

/// `HostError.files` (decision record 0003): left out when it names no file, and a list of
/// matter paths that round-trips when set, a code this version does not know included. A path no
/// matter could hold is refused, as everywhere a `MatterPath` is read.
#[test]
fn a_host_error_names_files_only_when_given_some() {
    let plain = HostError::new(
        ErrorCode::MatterFileLocked,
        "1 file open in another program",
    );
    assert_eq!(plain.files(), []);
    let json = serde_json::to_value(&plain).unwrap();
    assert_eq!(json.get("files"), None, "{json}");
    assert_eq!(serde_json::from_value::<HostError>(json).unwrap(), plain);

    let files = vec![
        MatterPath::new("Brief.docx").unwrap(),
        MatterPath::new("sub/Memo.docx").unwrap(),
    ];
    let named = plain.with_files(files.clone());
    let json = serde_json::to_value(&named).unwrap();
    assert_eq!(
        json["files"],
        json!(["Brief.docx", "sub/Memo.docx"]),
        "{json}"
    );
    let back: HostError = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back, named);
    assert_eq!(back.files(), files.as_slice());

    let mut unknown = json.clone();
    unknown["code"] = json!("matter.from_the_future");
    let back: HostError = serde_json::from_value(unknown).unwrap();
    assert_eq!(back.code(), ErrorCode::Unknown);
    assert_eq!(back.files(), files.as_slice());

    let mut bad = json;
    bad["files"] = json!(["../outside.docx"]);
    assert!(serde_json::from_value::<HostError>(bad).is_err());
}

/// `HostError.refusal` (decision record 0003): left out when unset; for `matter.file_refused`, the
/// cause the refused files share (left out when they differ), how many were refused and how many
/// files the action wrote, deleted or meant to, round-tripping with a code this version does not
/// know too. A cause from a newer Clauseo reads as `Unknown` and does not lose the error.
#[test]
fn a_host_error_carries_its_refusal_only_when_given_one() {
    let files = vec![
        MatterPath::new("Brief.docx").unwrap(),
        MatterPath::new("sub/Memo.docx").unwrap(),
    ];
    let plain = HostError::new(ErrorCode::MatterFileRefused, "matter: 2 of 3 files refused")
        .with_files(files.clone());
    assert!(plain.retryable());
    assert_eq!(plain.refusal(), None);
    let json = serde_json::to_value(&plain).unwrap();
    assert_eq!(json.get("refusal"), None, "{json}");

    let refusal = FileRefusal {
        because: Some(RefusedBecause::SecondName),
        refused: 2,
        of: 3,
    };
    let refused = plain.clone().with_refusal(refusal);
    let json = serde_json::to_value(&refused).unwrap();
    assert_eq!(
        json["refusal"],
        json!({"because": "second_name", "refused": 2, "of": 3}),
        "{json}"
    );
    let back: HostError = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back, refused);
    assert_eq!(back.refusal(), Some(refusal));

    let mixed = FileRefusal {
        because: None,
        ..refusal
    };
    let json_mixed = serde_json::to_value(plain.with_refusal(mixed)).unwrap();
    assert_eq!(json_mixed["refusal"], json!({"refused": 2, "of": 3}));
    let back: HostError = serde_json::from_value(json_mixed).unwrap();
    assert_eq!(back.refusal(), Some(mixed));

    for (because, text) in [
        (RefusedBecause::Permissions, "permissions"),
        (
            RefusedBecause::SavedCopyUnconfirmed,
            "saved_copy_unconfirmed",
        ),
    ] {
        let mut other = json.clone();
        other["refusal"]["because"] = json!(text);
        let back: HostError = serde_json::from_value(other).unwrap();
        assert_eq!(back.refusal().and_then(|r| r.because), Some(because));
    }

    let mut unknown = json.clone();
    unknown["code"] = json!("matter.from_the_future");
    let back: HostError = serde_json::from_value(unknown).unwrap();
    assert_eq!(back.code(), ErrorCode::Unknown);
    assert_eq!(back.refusal(), Some(refusal));

    let mut later = json;
    later["refusal"]["because"] = json!("a_later_cause");
    let back: HostError = serde_json::from_value(later).unwrap();
    assert_eq!(
        back.refusal().and_then(|r| r.because),
        Some(RefusedBecause::Unknown)
    );
    assert_eq!(back.files(), files.as_slice());
}

/// `HostError.write_failure` (decision record 0003): left out when unset; for
/// `matter.file_write_failed`, whether the file was put back and the action's file count,
/// round-tripping. `files` keeps at most `MAX_ERROR_FILES`.
#[test]
fn a_host_error_carries_its_write_failure_and_at_most_the_cap_of_files() {
    let lease = MatterPath::new("Lease.docx").unwrap();
    let plain = HostError::new(ErrorCode::MatterFileWriteFailed, "matter: put back")
        .with_files(vec![lease.clone()]);
    assert!(plain.retryable());
    assert_eq!(
        serde_json::to_value(&plain).unwrap().get("write_failure"),
        None
    );
    let failure = WriteFailure {
        put_back: false,
        of: 2,
    };
    let failed = plain.with_write_failure(failure);
    let json = serde_json::to_value(&failed).unwrap();
    assert_eq!(json["write_failure"], json!({"put_back": false, "of": 2}));
    let back: HostError = serde_json::from_value(json).unwrap();
    assert_eq!(back.write_failure(), Some(failure));
    assert_eq!(back.files(), [lease]);

    let many: Vec<MatterPath> = (0..MAX_ERROR_FILES + 3)
        .map(|n| MatterPath::new(&format!("f{n}.docx")).unwrap())
        .collect();
    let capped = HostError::new(ErrorCode::MatterFileLocked, "x").with_files(many.clone());
    assert_eq!(capped.files(), &many[..MAX_ERROR_FILES]);
}

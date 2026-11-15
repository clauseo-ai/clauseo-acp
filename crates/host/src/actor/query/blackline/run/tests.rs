//! stetdoc's environment (`stetdoc_env`): empty off Windows; on Windows `SystemRoot`, `windir`,
//! `TEMP` and `TMP`, the last two the blackline's own folder. And the code a failed run gives
//! (`error_of`, `refusal_code`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use clauseo_protocol::ErrorCode;

use super::{author_arg, error_of, refusal_code, stetdoc_env};

/// A lookup over `pairs`, the name matched in any case.
fn host(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| OsString::from(value))
    }
}

fn pairs(env: &BTreeMap<OsString, OsString>) -> Vec<(String, String)> {
    env.iter()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

#[test]
fn stetdoc_env_on_windows_is_system_root_windir_and_its_own_temp() {
    let dir = Path::new(r"C:\Data\tmp\blackline-1");
    let env = stetdoc_env(
        true,
        host(&[
            ("SYSTEMROOT", r"D:\Win"),
            ("TEMP", r"C:\Users\a\AppData\Local\Temp"),
            ("PATH", r"C:\bin"),
            ("SECRET_TOKEN", "x"),
        ]),
        || Some(OsString::from(r"C:\Windows")),
        dir,
    );
    assert_eq!(
        pairs(&env),
        vec![
            ("SystemRoot".to_owned(), r"D:\Win".to_owned()),
            ("TEMP".to_owned(), dir.display().to_string()),
            ("TMP".to_owned(), dir.display().to_string()),
            ("windir".to_owned(), r"C:\Windows".to_owned()),
        ]
    );
}

#[test]
fn stetdoc_env_off_windows_is_empty() {
    let env = stetdoc_env(
        false,
        host(&[("SystemRoot", r"D:\Win"), ("TEMP", "/tmp")]),
        || Some(OsString::from(r"C:\Windows")),
        Path::new("/data/tmp/blackline-1"),
    );
    assert!(env.is_empty());
}

#[test]
fn stetdoc_env_without_a_windows_directory_still_sets_temp() {
    let dir = Path::new(r"C:\Data\tmp\blackline-2");
    let env = stetdoc_env(true, host(&[]), || None, dir);
    assert_eq!(
        pairs(&env),
        vec![
            ("TEMP".to_owned(), dir.display().to_string()),
            ("TMP".to_owned(), dir.display().to_string()),
        ]
    );
}

#[test]
fn an_author_that_starts_with_a_dash_stays_one_argument() {
    assert_eq!(author_arg("-x"), OsString::from("--author=-x"));
    assert_eq!(author_arg("A. Mehta"), OsString::from("--author=A. Mehta"));
}

/// stetdoc 0.9.3's real answer to a compare whose inner table ends its cell and is removed
/// (`tests/fixtures/blackline/partial/compare-inner-last.json`).
const INNER_LAST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/blackline/partial/compare-inner-last.json"
));

#[test]
fn stetdoc_refusals_have_codes_of_their_own() {
    let (code, message) = error_of(INNER_LAST.as_bytes());
    assert_eq!(code, "validation_failed");
    assert_eq!(
        refusal_code(&code, &message),
        ErrorCode::BlacklineNestedTable
    );
    let code_of = |code: &str| refusal_code(code, "x");
    assert_eq!(
        code_of("validation_failed"),
        ErrorCode::BlacklineCannotCompare
    );
    assert_eq!(code_of("unsupported"), ErrorCode::BlacklineCannotCompare);
    for unread in [
        "malformed_input",
        "corrupt_complement",
        "unsupported_version",
    ] {
        assert_eq!(code_of(unread), ErrorCode::BlacklineNotReadable);
    }
    assert_eq!(code_of("limit_exceeded"), ErrorCode::BlacklineTooLarge);
    for other in ["io_error", "internal", "none", "zz_later"] {
        assert_eq!(code_of(other), ErrorCode::BlacklineFailed);
    }
    // No JSON, or a code that is not a plain word: `none`, and no message.
    assert_eq!(error_of(b"oops"), ("none".to_owned(), String::new()));
    let odd = br#"{"error":{"code":"a b","message":"m"}}"#;
    assert_eq!(error_of(odd), ("none".to_owned(), "m".to_owned()));
}

use super::*;
use crate::test_support::Scratch;

#[test]
fn a_run_is_written_to_marks_out_and_exits_0() {
    let scratch = Scratch::new("marks-run", &[]);
    let out = scratch.path().join("run.json");
    let run = r#"{"start_ms":812.5,"react_commits_per_frame":null}"#;
    assert_eq!(finish(Some(&out), Some(run), None), Ok(0));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), run);
}

#[test]
fn an_error_writes_nothing_and_exits_1() {
    let scratch = Scratch::new("marks-error", &[]);
    let out = scratch.path().join("run.json");
    let run = r#"{"start_ms":1}"#;
    assert_eq!(finish(Some(&out), Some(run), Some("no composer")), Ok(1));
    assert_eq!(finish(Some(&out), None, Some("no composer")), Ok(1));
    assert!(!out.exists());
}

#[test]
fn without_marks_out_or_a_json_run_it_fails_and_does_not_quit() {
    let scratch = Scratch::new("marks-refused", &[]);
    let out = scratch.path().join("run.json");
    for (out, run, error) in [
        (None, Some("{}"), None),
        (None, None, Some("boom")),
        (Some(out.as_path()), Some("not json"), None),
        (Some(out.as_path()), None, None),
    ] {
        let refused = finish(out, run, error).unwrap_err();
        assert_eq!(
            refused.code(),
            ErrorCode::CommandInvalid,
            "{run:?} {error:?}"
        );
    }
    assert!(!out.exists());
}

#[test]
fn a_write_that_fails_is_an_internal_error() {
    let scratch = Scratch::new("marks-unwritable", &[]);
    let out = scratch.path().join("no-such-folder").join("run.json");
    let refused = finish(Some(&out), Some("{}"), None).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::Internal);
}

#[cfg(target_os = "macos")]
#[test]
fn memory_is_a_positive_number_of_mib() {
    let mib = memory_mib().unwrap();
    assert!(mib > 1.0 && mib < 64.0 * 1024.0, "{mib}");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn memory_is_refused_off_macos() {
    assert!(memory_mib().is_err());
}

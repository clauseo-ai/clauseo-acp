//! Compare's answer read: stetdoc 0.9.3's real answers for a header in one version, a block it
//! could not mark up and a plain compare, and the answers that do not read.

use clauseo_protocol::BlacklineNotice::{HeaderOrFooterNotCompared, MayDiffer};

use super::{CompareSaid, read_compare};

/// A fixture of `tests/fixtures/blackline/partial/`, as text.
macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/blackline/partial/",
            $name
        ))
    };
}

/// An answer whose receipt warns `codes`.
fn warning(codes: &[&str]) -> String {
    let warnings: Vec<serde_json::Value> = codes
        .iter()
        .map(|code| serde_json::json!({"code": code, "message": "The rent is 5."}))
        .collect();
    serde_json::json!({"receipt": {"warnings": warnings, "text": "x"}}).to_string()
}

#[test]
fn a_header_in_one_version_is_noticed() {
    let said = read_compare(fixture!("compare-unmatched.json"));
    assert_eq!(said.notices, [HeaderOrFooterNotCompared]);
    assert_eq!(said.codes, ["compare_story_unmatched"]);
}

#[test]
fn a_block_compare_could_not_mark_up_may_differ() {
    let said = read_compare(fixture!("compare-excluded.json"));
    assert_eq!(said.notices, [MayDiffer]);
    assert_eq!(said.codes, ["compare_not_restored"]);
    assert_eq!(
        read_compare(&warning(&["compare_not_redlined"])).notices,
        [MayDiffer]
    );
}

#[test]
fn a_plain_compare_says_nothing() {
    assert_eq!(
        read_compare(fixture!("compare-stories.json")),
        CompareSaid::default()
    );
    let shown = warning(&[
        "compare_revisions_accepted",
        "compare_paragraph_replaced",
        "compare_table_replaced",
        "compare_keys_merged",
        "repaired",
    ]);
    assert_eq!(read_compare(&shown).notices, []);
}

/// Something compare could not carry, a code this version does not know, and one that is not a
/// plain word (logged as `other`, never as its text) may differ.
#[test]
fn any_other_warning_may_differ() {
    for code in ["compare_objects_dropped", "zz_later", "The rent is 5."] {
        let said = read_compare(&warning(&[code]));
        assert_eq!(said.notices, [MayDiffer], "{code}");
    }
    assert_eq!(read_compare(&warning(&["The rent is 5."])).codes, ["other"]);
}

/// An answer that does not read is never "nothing to say".
#[test]
fn an_answer_that_does_not_read_may_differ() {
    for json in [
        "{}",
        "",
        "not json",
        r#"{"receipt":{}}"#,
        r#"{"receipt":{"warnings":[{}]}}"#,
    ] {
        let said = read_compare(json);
        assert_eq!(said.notices, [MayDiffer], "{json}");
        assert_eq!(said.codes, ["unreadable"], "{json}");
    }
}

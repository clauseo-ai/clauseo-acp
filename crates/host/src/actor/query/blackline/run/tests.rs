//! stetdoc's environment (`stetdoc_env`): empty off Windows; on Windows `SystemRoot`, `windir`,
//! `TEMP` and `TMP`, the last two the blackline's own folder. And the code a failed run gives
//! (`error_of`, `refusal_code`). What the blackline says of a difference it does not show, of what
//! the versions' views found, and of what the table comparison found.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use clauseo_protocol::ErrorCode;

use clauseo_protocol::{
    BlacklineNotice, BlacklineParagraph, BlacklinePart, BlacklinePartKind, BlacklineRun,
    ChangedTable, ParagraphChange, RunKind, TableFormat, TablePlace, WordBlackline,
};

use super::cross::{LINKS_MAX, note_what_compare_missed};
use super::{
    TablesCompared, author_arg, error_of, note_a_silent_difference, note_tables, refusal_code,
    stetdoc_env,
};
use crate::actor::query::blackline::parse::sides::Crossed;

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

/// Two files that differ, blacklined with no change shown and nothing said (a link's address
/// changed, which stetdoc does not mark), say the blackline may differ; the same files, or a
/// change shown, or a notice that already says part may be missing, add nothing.
#[test]
fn a_difference_the_blackline_does_not_show_may_differ() {
    let blackline = |change| WordBlackline {
        paragraphs: vec![BlacklineParagraph {
            change,
            runs: Vec::new(),
            hidden: false,
        }],
        truncated: false,
        notices: Vec::new(),
        changed_links: Vec::new(),
        parts: Vec::new(),
        changed_tables: Vec::new(),
    };
    let noted = |mut blackline: WordBlackline, differ| {
        note_a_silent_difference(&mut blackline, differ);
        blackline.notices
    };
    assert_eq!(
        noted(blackline(ParagraphChange::Same), true),
        [BlacklineNotice::MayDiffer]
    );
    assert_eq!(noted(blackline(ParagraphChange::Same), false), []);
    assert_eq!(noted(blackline(ParagraphChange::Changed), true), []);
    // A notice about something else (the same hidden word in both) does not say they differ.
    let mut hidden = blackline(ParagraphChange::Same);
    hidden.notices.push(BlacklineNotice::HiddenText);
    assert_eq!(
        noted(hidden.clone(), true),
        [BlacklineNotice::HiddenText, BlacklineNotice::MayDiffer]
    );
    assert_eq!(noted(hidden, false), [BlacklineNotice::HiddenText]);
    // One that already says part may be missing, or names what changed, does.
    for said in [
        BlacklineNotice::PartNotShown,
        BlacklineNotice::ChangesOutsideText,
        BlacklineNotice::HeaderOrFooterNotCompared,
        BlacklineNotice::CommentChanged,
        BlacklineNotice::TextBoxChanged,
        BlacklineNotice::CellFormatChanged,
    ] {
        let mut blackline = blackline(ParagraphChange::Same);
        blackline.notices.push(said);
        assert_eq!(noted(blackline, true), [said]);
    }
    let mut linked = blackline(ParagraphChange::Same);
    linked.changed_links.push("the account".to_owned());
    assert_eq!(noted(linked, true), []);
    // A part shown, or a table named, says what changed.
    let mut parted = blackline(ParagraphChange::Same);
    parted.parts.push(BlacklinePart {
        kind: BlacklinePartKind::Footnote { number: 1 },
        paragraphs: Vec::new(),
    });
    assert_eq!(noted(parted, true), []);
    let mut tabled = blackline(ParagraphChange::Same);
    tabled.changed_tables.push(shaded_table());
    assert_eq!(noted(tabled, true), []);
}

/// The body's first table, its shading changed.
fn shaded_table() -> ChangedTable {
    ChangedTable {
        place: TablePlace::Body,
        number: 1,
        formats: vec![TableFormat::Shading],
    }
}

/// A header's text one version only has says the header was not compared, unless a part the
/// blackline shows holds it (in its older or its newer text), the notice is there already, or
/// the headers' view did not read.
#[test]
fn a_header_text_one_version_only_has_is_said_unless_shown() {
    let lone = |texts: &[&str]| TablesCompared {
        lone_header_texts: texts.iter().map(|text| (*text).to_owned()).collect(),
        ..TablesCompared::default()
    };
    let empty = WordBlackline {
        paragraphs: Vec::new(),
        truncated: false,
        notices: Vec::new(),
        changed_links: Vec::new(),
        parts: Vec::new(),
        changed_tables: Vec::new(),
    };
    let mut unshown = empty.clone();
    note_tables(&mut unshown, lone(&["Scheduleheader."]), true);
    assert_eq!(
        unshown.notices,
        [BlacklineNotice::HeaderOrFooterNotCompared]
    );
    // When the headers' view did not read, stetdoc's list of changes says what is not shown.
    let mut unread = empty.clone();
    note_tables(&mut unread, lone(&["Scheduleheader."]), false);
    assert_eq!(unread.notices, []);
    let mut shown = empty.clone();
    let run = |kind, text: &str| BlacklineRun {
        kind,
        text: text.to_owned(),
        object: None,
    };
    shown.parts.push(BlacklinePart {
        kind: BlacklinePartKind::Header { uses: Vec::new() },
        paragraphs: vec![BlacklineParagraph {
            change: ParagraphChange::Changed,
            runs: vec![
                run(RunKind::Same, "Header "),
                run(RunKind::Del, "one"),
                run(RunKind::Ins, "two"),
                run(RunKind::Same, ".\t"),
            ],
            hidden: false,
        }],
    });
    note_tables(&mut shown, lone(&["Headerone.", "Headertwo."]), true);
    assert_eq!(shown.notices, []);
    let mut said = empty;
    said.notices
        .push(BlacklineNotice::HeaderOrFooterNotCompared);
    note_tables(&mut said, lone(&["Scheduleheader."]), true);
    assert_eq!(said.notices, [BlacklineNotice::HeaderOrFooterNotCompared]);
}

/// What the comparison of the table formatting found goes on the blackline: the tables with the
/// table notice, and may differ where it could not tell, in order; nothing found adds nothing.
#[test]
fn what_the_table_comparison_found_is_said() {
    let empty = WordBlackline {
        paragraphs: Vec::new(),
        truncated: false,
        notices: vec![BlacklineNotice::HiddenText],
        changed_links: Vec::new(),
        parts: Vec::new(),
        changed_tables: Vec::new(),
    };
    let mut found = empty.clone();
    note_tables(
        &mut found,
        TablesCompared {
            changed: vec![shaded_table()],
            may_differ: true,
            ..TablesCompared::default()
        },
        true,
    );
    assert_eq!(found.changed_tables, [shaded_table()]);
    assert_eq!(
        found.notices,
        [
            BlacklineNotice::HiddenText,
            BlacklineNotice::CellFormatChanged,
            BlacklineNotice::MayDiffer,
        ]
    );
    let mut none = empty.clone();
    note_tables(&mut none, TablesCompared::default(), true);
    assert_eq!(none, empty);
}

/// What the versions' views found goes on the blackline: the links by text, to the cap (past it,
/// may differ), a link with no text as may differ, and a comment's and a text box's notices, in
/// order.
#[test]
fn what_compare_missed_is_said() {
    let empty = WordBlackline {
        paragraphs: Vec::new(),
        truncated: false,
        notices: vec![BlacklineNotice::MayDiffer, BlacklineNotice::HiddenText],
        changed_links: Vec::new(),
        parts: Vec::new(),
        changed_tables: Vec::new(),
    };
    let mut said = empty.clone();
    note_what_compare_missed(
        &mut said,
        Crossed {
            links: vec!["the account".to_owned()],
            unnamed_link: false,
            comments: true,
            boxes: true,
        },
    );
    assert_eq!(said.changed_links, ["the account"]);
    assert_eq!(
        said.notices,
        [
            BlacklineNotice::HiddenText,
            BlacklineNotice::CommentChanged,
            BlacklineNotice::TextBoxChanged,
            BlacklineNotice::MayDiffer,
        ]
    );
    let mut many = WordBlackline {
        notices: Vec::new(),
        ..empty
    };
    let links: Vec<String> = (0..=LINKS_MAX).map(|n| format!("link {n}")).collect();
    note_what_compare_missed(
        &mut many,
        Crossed {
            links,
            ..Crossed::default()
        },
    );
    assert_eq!(many.changed_links.len(), LINKS_MAX);
    assert_eq!(many.notices, [BlacklineNotice::MayDiffer]);
    let mut unnamed = WordBlackline {
        notices: Vec::new(),
        ..many.clone()
    };
    unnamed.changed_links.clear();
    note_what_compare_missed(
        &mut unnamed,
        Crossed {
            unnamed_link: true,
            ..Crossed::default()
        },
    );
    assert_eq!(unnamed.notices, [BlacklineNotice::MayDiffer]);
}

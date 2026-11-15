//! The reader on stetdoc 0.9.0's real output: the fixture's redline (`view.json`), lines it
//! printed for the other markup it knows, the escapes, and what it leaves out. stetdoc 0.9.3's
//! nested tables, hidden text and long lines, and the partial reads, are in `partial.rs`.

use clauseo_protocol::{
    BlacklineNotice, BlacklineParagraph, BlacklineRun, ParagraphChange, RunKind,
};

use super::{Unreadable, ViewFailure, read_text, read_view};

mod forms;
mod partial;

/// `stetdoc view --all --json` on the fixture's redline (`tests/fixtures/blackline/`).
const VIEW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/blackline/view.json"
));

/// A paragraph of `change` with `runs`, each `(kind, text)`.
pub(super) fn para(change: ParagraphChange, runs: &[(RunKind, &str)]) -> BlacklineParagraph {
    BlacklineParagraph {
        hidden: false,
        change,
        runs: runs
            .iter()
            .map(|&(kind, text)| BlacklineRun {
                kind,
                text: text.to_owned(),
                object: None,
            })
            .collect(),
    }
}

use ParagraphChange::{Added, Changed, Formatted, Removed, Same as Kept};

/// The paragraph that stands where a line was left out.
pub(super) fn gap() -> BlacklineParagraph {
    para(ParagraphChange::NotShown, &[])
}
use RunKind::{Del, Ins, Same};

#[test]
fn the_fixture_reads_paragraph_by_paragraph() {
    let read = read_view(VIEW).unwrap();
    assert_eq!((read.skipped, read.warnings.len()), (0, 0));
    let blackline = read.blackline;
    assert!(!blackline.truncated);
    // The footnote changed too, and the view does not mark it.
    assert_eq!(blackline.notices, [BlacklineNotice::ChangesOutsideText]);
    let expected = vec![
        para(Kept, &[(Same, "Lease Agreement")]),
        para(
            Changed,
            &[
                (Same, "The Tenant shall pay rent of $1,"),
                (Del, "000 monthly"),
                (Ins, "200 each month in advance"),
                (Same, "."),
            ],
        ),
        para(Removed, &[(Del, "The deposit is returned within 30 days.")]),
        para(
            Changed,
            &[
                (Same, "Governing law is "),
                (Del, "New York"),
                (Ins, "Delaware"),
                (Same, "."),
            ],
        ),
        para(Kept, &[(Same, "1. Pay on time.")]),
        para(
            Changed,
            &[
                (Same, "2. Keep the premises clean"),
                (Ins, " and tidy"),
                (Same, "."),
            ],
        ),
        para(Kept, &[(Same, "Rent")]),
        para(Changed, &[(Same, "$1,"), (Del, "000"), (Ins, "200")]),
        para(Kept, &[(Same, "Term")]),
        para(Kept, &[(Same, "12 months")]),
        para(Kept, &[(Same, "A notice period applies.")]),
        para(
            Changed,
            &[
                (Same, "Odd {"),
                (Del, "braces"),
                (Ins, "curly"),
                (Same, "} and back\\slash and\ttab here."),
            ],
        ),
        // A format change only: its text is the same.
        para(Formatted, &[(Same, "This clause is important.")]),
        para(Added, &[(Ins, "Notices go by email.")]),
        para(Kept, &[(Same, "Signatures")]),
    ];
    assert_eq!(blackline.paragraphs, expected);
}

/// Lines stetdoc 0.9.0 printed for a redline of documents with a link, a comment, a field, a
/// page break and a line break, a bookmark, a content control, a numbered heading, an empty
/// paragraph, a moved paragraph, soft hyphens and an inserted table row.
const MARKUP: &str = r#"{stet 1 view=body base=134e2b4c96a735f3:0:134e2b4c96a735f3 mode=redline common-style="Normal" range=p1..t1}
{authors: C "Clauseo"}
{warning read_anomaly p17: content exists that is not shown: a paragraph or table inside a paragraph is not shown (2 times) · next: check the document with the check command}
p2 See {link "https://example.com"}the {link "https://example.com"}{--site--}{#21 C}{/link}{++website++}{#20 C}{/link} now.
p3 {c0}Commented {--words--}{#23 C}{++phrase++}{#22 C}{/c0} after.
p4 Page {--1--}{#26 C}{++{field PAGE "2"}++}{#25 C}
p5 Before break{pagebreak}after {++the ++}{#27 C}break\nline two
p8 {ctl rich "Party"}Acme {--Ltd--}{#29 C}{++Limited++}{#28 C}{/ctl}
p9 {style "heading 1"} {§"1."} Clause {--one--}{#31 C}{++first++}{#30 C}
p10
p14 soft\u{ad}hyphen and‑nb\u{f0e0}
t1 {table cols=2}
r1 {row inserted #16 C}
p16 {¶+ #18 C} {++A++}{#17 C}
p17 {¶+ #19 C}
{/table}
p3 {§"1." ghost} {¶- #2 RT} {--Item a--}{#1 RT}
p1 {img "A \"q\" \} \{ \\ x"}Logo {--line--}{#2 RT}{++row++}{#1 RT}
p2 F: {field REF start "res \"a\" \}\{ b"}
{section 1 ends: next-page}
{appendix}
{c0 J 2026-01-01 on p3 "Commented phrase"} Check this
p99 not read: the appendix is skipped
{position: p1..t1 of p1..t1 (16 of 16 blocks)}
"#;

#[test]
fn the_other_markup_reads_as_its_text() {
    let expected = vec![
        para(
            Changed,
            &[
                (Same, "See the "),
                (Del, "site"),
                (Ins, "website"),
                (Same, " now."),
            ],
        ),
        para(
            Changed,
            &[
                (Same, "Commented "),
                (Del, "words"),
                (Ins, "phrase"),
                (Same, " after."),
            ],
        ),
        para(Changed, &[(Same, "Page "), (Del, "1"), (Ins, "2")]),
        para(
            Changed,
            &[
                (Same, "Before break\nafter "),
                (Ins, "the "),
                (Same, "break\nline two"),
            ],
        ),
        para(Changed, &[(Same, "Acme "), (Del, "Ltd"), (Ins, "Limited")]),
        para(
            Changed,
            &[(Same, "1. Clause "), (Del, "one"), (Ins, "first")],
        ),
        para(Kept, &[]),
        para(Kept, &[(Same, "soft\u{ad}hyphen and\u{2011}nb\u{f0e0}")]),
        para(Added, &[(Ins, "A")]),
        para(Added, &[]),
        para(Removed, &[(Del, "1. Item a")]),
        para(Changed, &[(Same, "Logo "), (Del, "line"), (Ins, "row")]),
        para(Kept, &[(Same, "F: res \"a\" }{ b")]),
    ];
    let read = read_text(MARKUP);
    assert_eq!((read.skipped, read.seen.unknown), (0, false));
    assert_eq!(read.paragraphs, expected);
}

#[test]
fn escaped_markup_is_document_text() {
    let line = r"p1 Odd \{++x++\}\{#3 A\} and \{--{--y--}{#1 RT}{++z++}{#0 RT}--\} and \\ back \{¶+ #1 C\} & tab\there";
    assert_eq!(
        read_text(line).paragraphs,
        vec![para(
            Changed,
            &[
                (Same, "Odd {++x++}{#3 A} and {--"),
                (Del, "y"),
                (Ins, "z"),
                (Same, "--} and \\ back {¶+ #1 C} & tab\there"),
            ],
        )]
    );
    // An inserted "C++" ends at the last "++}".
    assert_eq!(
        read_text("p1 {++C++++}{#1 A}").paragraphs,
        vec![para(Changed, &[(Ins, "C++")])]
    );
}

/// A line that breaks a rule is left out, by its line and rule alone, and the lines around it
/// are read.
#[test]
fn a_line_that_breaks_a_rule_is_left_out() {
    let skips = |text: &str, why: &str| {
        let read = read_text(&format!("p1 before\n{text}\np3 after"));
        assert_eq!(read.skipped, 1, "{text}");
        assert_eq!(
            read.first_skip.map(|at| (at.line, at.why)),
            Some((2, why)),
            "{text}"
        );
        assert_eq!(
            read.paragraphs,
            [
                para(Kept, &[(Same, "before")]),
                gap(),
                para(Kept, &[(Same, "after")])
            ],
            "{text}"
        );
    };
    skips("p2 a } b", "a closing brace that is not escaped");
    skips(r"p2 a \q b", "an escape the reader does not know");
    skips("p2 {++new++} after", "a change without its id");
    skips("p2 {--old--}{note}", "a change without its id");
    skips(
        "p2 {++never ends",
        "an insertion or deletion that does not end",
    );
    skips("p2 {link \"x\"}a{/link {b}}", "markup inside markup");
    skips("p2 {field PAGE}", "a field with no result");
    skips("p2 {hidden}", "a hidden-text marker with no text");
    skips("p2 a {unclosed", "markup that does not end");
    skips(
        "p2 a {\"quoted\"}",
        "a marker that does not start with a word",
    );
    skips(
        "x2 a block of another kind",
        "a block of a kind the reader does not know",
    );
    skips("t2 no marker", "a table or row line without its marker");
    skips("just text", "a line with no block ref");
    skips("p2x text", "a line with no block ref");
    skips("r2 {row}", "a row outside a table");
    skips("{/table}", "a table end with no table open");
}

/// Markup a newer stetdoc may add: a marker whose first word is not known is left out and
/// noted, words after the ones a known marker needs are ignored, and nothing else changes.
#[test]
fn markup_from_a_newer_stetdoc_is_left_out_or_read_in_part() {
    let read = read_text(concat!(
        "p1 a {zz \"secret\"} b\n",
        "p2 {field REF \"4.2\" ->p57 label} and {field PAGE \"2\" \"the label\" newword}\n",
        "p3 {§\"1.\" moved} {link \"x\" more}site{/link} {img \"logo\" spin=3}\n",
        "p4 {style \"Body\" next=\"x\"} {¶+ #1 C later} {++new++}{#1 C move from #3}\n",
        "t1 {table cols=1 repeat=header}\n",
        "r1 {row header repeating}\n",
        "p5 {span 2 wide} {vmerge restart more} cell\n",
        "r2 {repeat-row #7 C}\n",
        "{/table}",
    ));
    assert_eq!(read.skipped, 0);
    assert!(read.seen.unknown);
    assert_eq!(
        read.paragraphs,
        [
            para(Kept, &[(Same, "a  b")]),
            para(Kept, &[(Same, "4.2 and 2")]),
            para(Kept, &[(Same, "1. site ")]),
            para(Added, &[(Ins, "new")]),
            para(Kept, &[(Same, "cell")]),
        ]
    );
    // The known tags and markers alone note nothing.
    assert!(
        !read_text("p1 {§\"1.\"} {•} {joins-next} x{c1}{/c1}{fn.}{en2}{page}")
            .seen
            .unknown
    );
}

#[test]
fn the_json_around_the_text_is_checked() {
    let view = |mode: &str, next: &str, shown: u32| {
        serde_json::json!({
            "mode": mode,
            "next": serde_json::from_str::<serde_json::Value>(next).unwrap(),
            "position": {"shown": shown, "total": 4},
            "text": "{stet 1 view=body mode=redline}\np1 a",
        })
        .to_string()
    };
    let truncated = |json: &str| read_view(json).unwrap().blackline.truncated;
    assert!(!truncated(&view("redline", "null", 4)));
    // stetdoc says the page is partial: fewer blocks shown, or a next page named.
    assert!(truncated(&view("redline", "null", 3)));
    assert!(truncated(&view("redline", "\"--after p2\"", 4)));
    let not = |json: &str, why: &'static str| {
        assert_eq!(
            read_view(json).unwrap_err(),
            ViewFailure::NotAView(Unreadable { line: 0, why })
        );
    };
    not(&view("final", "null", 4), "the view is not a redline");
    not(
        r#"{"error": {"code": "malformed_input", "message": "x"}}"#,
        "the JSON is not a view",
    );
    not("not json", "the JSON is not a view");
}

/// A view that warns, as stetdoc 0.9.0 did for a table nested in a cell (content not shown), is
/// read whole, its warnings notices by their codes; a code that is not a plain word is logged as
/// `other` and says only that the blackline may differ.
#[test]
fn a_view_with_a_warning_is_read_with_notices() {
    let mut view: serde_json::Value = serde_json::from_str(VIEW).unwrap();
    view["warnings"] = serde_json::json!([
        {
            "block": "p17",
            "code": "read_anomaly",
            "message": "content exists that is not shown: a paragraph or table inside a paragraph is not shown (2 times)",
            "next": "check the document with the check command",
            "story": "body"
        },
        {"code": "Secret Text {x}", "message": "x"},
        {"code": "unreferenced_note", "message": "x"},
        {"code": "own_text_removed", "message": "x"},
        {"code": "property_change_folded", "message": "x"}
    ]);
    let read = read_view(&view.to_string()).unwrap();
    assert_eq!(
        read.blackline.paragraphs,
        read_view(VIEW).unwrap().blackline.paragraphs
    );
    // The fixture's footnote change, outside the main text, is noted as before.
    assert_eq!(
        read.blackline.notices,
        [
            BlacklineNotice::PartNotShown,
            BlacklineNotice::ChangesOutsideText,
            BlacklineNotice::MayDiffer
        ]
    );
    assert_eq!(
        read.warnings,
        [
            "read_anomaly",
            "other",
            "unreferenced_note",
            "own_text_removed",
            "property_change_folded"
        ]
    );
    // `own_text_removed` and `property_change_folded` each say the blackline may differ;
    // `unreferenced_note` says nothing.
    for (code, may_differ) in [
        ("own_text_removed", true),
        ("property_change_folded", true),
        ("unreferenced_note", false),
    ] {
        view["warnings"] = serde_json::json!([{"code": code, "message": "x"}]);
        let notices = read_view(&view.to_string()).unwrap().blackline.notices;
        assert_eq!(
            notices.contains(&BlacklineNotice::MayDiffer),
            may_differ,
            "{code}"
        );
    }
}

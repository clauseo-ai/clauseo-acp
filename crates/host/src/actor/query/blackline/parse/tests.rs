//! The reader on stetdoc 0.9.0's real output: the fixture's redline (`view.json`), lines it
//! printed for the other markup it knows, the escapes, and the failures.

use clauseo_protocol::{BlacklineParagraph, BlacklineRun, ParagraphChange, RunKind};

use super::{Unreadable, ViewFailure, read_text, read_view};

/// `stetdoc view --all --json` on the fixture's redline (`tests/fixtures/blackline/`).
const VIEW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/blackline/view.json"
));

/// A paragraph of `change` with `runs`, each `(kind, text)`.
fn para(change: ParagraphChange, runs: &[(RunKind, &str)]) -> BlacklineParagraph {
    BlacklineParagraph {
        change,
        runs: runs
            .iter()
            .map(|&(kind, text)| BlacklineRun {
                kind,
                text: text.to_owned(),
            })
            .collect(),
    }
}

use ParagraphChange::{Added, Changed, Removed, Same as Kept};
use RunKind::{Del, Ins, Same};

#[test]
fn the_fixture_reads_paragraph_by_paragraph() {
    let blackline = read_view(VIEW).unwrap();
    assert!(!blackline.truncated);
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
        para(Kept, &[(Same, "This clause is important.")]),
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
    assert_eq!(read_text(MARKUP).unwrap(), expected);
}

#[test]
fn escaped_markup_is_document_text() {
    let line = r"p1 Odd \{++x++\}\{#3 A\} and \{--{--y--}{#1 RT}{++z++}{#0 RT}--\} and \\ back \{¶+ #1 C\} & tab\there";
    assert_eq!(
        read_text(line).unwrap(),
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
        read_text("p1 {++C++++}{#1 A}").unwrap(),
        vec![para(Changed, &[(Ins, "C++")])]
    );
}

#[test]
fn what_the_reader_does_not_know_fails_the_read() {
    let fails = |text: &str, why: &str| {
        let error = read_text(text).unwrap_err();
        assert_eq!(error.why, why, "{text}");
    };
    fails("p1 a {zz} b", "markup the reader does not know");
    fails("p1 a {#3 C} b", "markup the reader does not know");
    fails("p1 a {¶+ #1 C} b", "markup the reader does not know");
    fails("p1 a {rev run #1 C} b", "markup the reader does not know");
    fails("p1 a } b", "a closing brace that is not escaped");
    fails(r"p1 a \q b", "an escape the reader does not know");
    fails("p1 {++new++} after", "a change without its id");
    fails("p1 {--old--}{note}", "a change without its id");
    fails(
        "p1 {++never ends",
        "an insertion or deletion that does not end",
    );
    fails(
        "p1 {--a {++b++}{#1 C}--}{#2 C}",
        "markup the reader does not know",
    );
    fails(
        "p1 {link \"x\" more}a{/link}",
        "a link or image marker the reader does not know",
    );
    fails("p1 {field PAGE}", "a field with no result");
    fails(
        "p1 {§\"1.\" moved} a",
        "a list number the reader does not know",
    );
    fails("p1 a {unclosed", "markup that does not end");
    fails(
        "x1 a block of another kind",
        "a block of a kind the reader does not know",
    );
    fails("t1 no marker", "a table or row line without its marker");
    fails("just text", "a line with no block ref");
    fails("p1x text", "a line with no block ref");
    assert_eq!(
        read_text("p1 ok\np2 a {zz}").unwrap_err(),
        Unreadable {
            line: 2,
            why: "markup the reader does not know"
        }
    );
}

#[test]
fn the_json_around_the_text_is_checked() {
    let view = |mode: &str, next: &str, shown: u32| {
        serde_json::json!({
            "mode": mode,
            "next": serde_json::from_str::<serde_json::Value>(next).unwrap(),
            "position": {"shown": shown, "total": 4},
            "text": "p1 a",
        })
        .to_string()
    };
    assert!(!read_view(&view("redline", "null", 4)).unwrap().truncated);
    // stetdoc says the page is partial: fewer blocks shown, or a next page named.
    assert!(read_view(&view("redline", "null", 3)).unwrap().truncated);
    assert!(
        read_view(&view("redline", "\"--after p2\"", 4))
            .unwrap()
            .truncated
    );
    let not = |json: &str, why: &'static str| {
        assert_eq!(
            read_view(json).unwrap_err(),
            ViewFailure::Unreadable(Unreadable { line: 0, why })
        );
    };
    not(&view("final", "null", 4), "the view is not a redline");
    not(
        r#"{"error": {"code": "malformed_input", "message": "x"}}"#,
        "the JSON is not a view",
    );
    not("not json", "the JSON is not a view");
}

/// A view that warns, as stetdoc 0.9.0 did for a table nested in a cell (content not shown),
/// fails by its codes alone, however well its text reads; a code that is not a plain word is
/// logged as `other`.
#[test]
fn a_view_with_a_warning_fails_by_its_codes() {
    let mut view: serde_json::Value = serde_json::from_str(VIEW).unwrap();
    view["warnings"] = serde_json::json!([
        {
            "block": "p17",
            "code": "read_anomaly",
            "message": "content exists that is not shown: a paragraph or table inside a paragraph is not shown (2 times)",
            "next": "check the document with the check command",
            "story": "body"
        },
        {"code": "Secret Text {x}", "message": "x"}
    ]);
    let failure = read_view(&view.to_string()).unwrap_err();
    assert_eq!(
        failure,
        ViewFailure::Warned(vec!["read_anomaly".to_owned(), "other".to_owned()])
    );
    assert_eq!(failure.to_string(), "it warned: read_anomaly, other");
}

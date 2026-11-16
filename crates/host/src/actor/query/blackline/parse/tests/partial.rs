//! The reader on stetdoc 0.9.3's real output for tables inside tables, hidden text and a long
//! paragraph (`tests/fixtures/blackline/partial/`), and the reads that show a part: a line
//! left out, a view with no line that reads, a format it does not know, and the depth caps.

use clauseo_protocol::BlacklineNotice::{ChangesOutsideText, HiddenText, MayDiffer, PartNotShown};
use clauseo_protocol::ParagraphChange::{Added, Changed, Formatted, Removed, Same as Kept};
use clauseo_protocol::RunKind::{Del, Ins, Same};
use clauseo_protocol::{BlacklineObject, BlacklineParagraph, BlacklineRun, RunKind};

use super::super::line::MAX_CHANGE_DEPTH;
use super::super::{MAX_TABLE_DEPTH, Read, ViewFailure, read_text, read_view};
use super::{gap, para};

/// `para` marked as holding hidden text.
fn hidden(mut paragraph: BlacklineParagraph) -> BlacklineParagraph {
    paragraph.hidden = true;
    paragraph
}

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

/// The view `json` read.
fn read(json: &str) -> Read {
    read_view(json).unwrap()
}

/// `view` with its text changed by `change`.
fn with_text(view: &str, change: impl FnOnce(&str) -> String) -> String {
    let mut json: serde_json::Value = serde_json::from_str(view).unwrap();
    let text = change(json["text"].as_str().unwrap());
    json["text"] = serde_json::Value::String(text);
    json.to_string()
}

/// The term paragraph the pairs change: "one year" to "two years".
fn term(before: &str) -> BlacklineParagraph {
    para(
        Changed,
        &[
            (Same, before),
            (Del, "one year"),
            (Ins, "two years"),
            (Same, "."),
        ],
    )
}

/// The outer table's cells, in order: the inner table's cell between the outer left cell's
/// paragraphs.
#[test]
fn a_table_inside_a_cell_reads_as_its_paragraphs_in_order() {
    let read = read(fixture!("view-nested.json"));
    assert_eq!((read.skipped, read.warnings.len()), (0, 0));
    assert_eq!(read.blackline.notices, []);
    assert_eq!(
        read.blackline.paragraphs,
        [
            term("The term is "),
            para(Kept, &[(Same, "Outer left.")]),
            para(Kept, &[(Same, "Inner cell.")]),
            para(Kept, &[]),
            para(Kept, &[(Same, "Outer right.")]),
            para(Kept, &[(Same, "Last paragraph.")]),
        ]
    );
}

#[test]
fn a_change_inside_an_inner_table_is_shown() {
    let read = read(fixture!("view-nested-inner.json"));
    assert_eq!(read.blackline.notices, []);
    let paragraphs = read.blackline.paragraphs;
    assert_eq!(paragraphs.len(), 6);
    assert_eq!(paragraphs[2], term("Inner cell "));
}

#[test]
fn tables_three_deep_read_to_the_deepest() {
    let read = read(fixture!("view-deep3.json"));
    assert_eq!(read.skipped, 0);
    let texts: Vec<_> = read
        .blackline
        .paragraphs
        .iter()
        .map(|p| p.runs.len())
        .collect();
    assert_eq!(texts, [1, 1, 1, 4, 0, 0]);
    assert_eq!(read.blackline.paragraphs[3], term("Deep "));
}

/// Hidden text is shown as stetdoc's view shows it, as document content, with a notice; its
/// warning gives the same notice once.
#[test]
fn hidden_text_is_shown_with_a_notice() {
    let read = read(fixture!("view-hidden.json"));
    assert_eq!(read.warnings, ["hidden_text"]);
    assert_eq!(read.blackline.notices, [HiddenText]);
    assert_eq!(
        read.blackline.paragraphs,
        [
            term("The term is "),
            hidden(para(Kept, &[(Same, "Visible hidden words")])),
        ]
    );
    let changed = self::read(fixture!("view-hidden-change.json"));
    assert_eq!(changed.blackline.notices, [HiddenText]);
    assert_eq!(
        changed.blackline.paragraphs[1],
        hidden(para(
            Changed,
            &[
                (Same, "Visible hidden "),
                (Del, "one year"),
                (Ins, "two years"),
            ]
        ))
    );
    assert!(!changed.blackline.paragraphs[0].hidden);
}

/// stetdoc wraps a paragraph past about 1,500 characters with a trailing backslash: the lines
/// are one paragraph, the backslashes gone.
#[test]
fn a_wrapped_paragraph_reads_as_one() {
    let read = read(fixture!("view-long.json"));
    assert_eq!((read.skipped, read.blackline.paragraphs.len()), (0, 2));
    let words: Vec<String> = (0..600).map(|i| format!("word{i}")).collect();
    assert_eq!(
        read.blackline.paragraphs[1],
        para(Changed, &[(Same, &words.join(" ")), (Ins, " end")])
    );
}

/// A line that does not read, in the middle of a real view, is left out: every other block is
/// shown, with the notice that a part is not.
#[test]
fn a_line_that_does_not_read_leaves_the_rest() {
    let whole = read(fixture!("view-nested-inner.json"))
        .blackline
        .paragraphs;
    let view = with_text(fixture!("view-nested-inner.json"), |text| {
        text.replace("p3 Inner cell ", "p3 Inner } cell ")
    });
    let read = read_view(&view).unwrap();
    assert_eq!(read.skipped, 1);
    assert_eq!(read.blackline.notices, [PartNotShown]);
    let mut rest = whole;
    rest[2] = gap();
    assert_eq!(read.blackline.paragraphs, rest);
    // Its line and rule go to the log, no text.
    let first = read.first_skip.unwrap();
    assert_eq!(
        (first.line, first.why),
        (10, "a closing brace that is not escaped")
    );
}

/// Content stetdoc collapses (`{opaque …}`) is not shown, and says so.
#[test]
fn an_opaque_line_is_a_part_not_shown() {
    let view = with_text(fixture!("view-nested.json"), |text| {
        text.replace("p6 Last", "{opaque toc p7..p9 3 paragraphs}\np6 Last")
    });
    let read = read_view(&view).unwrap();
    assert_eq!((read.skipped, read.blackline.paragraphs.len()), (0, 6));
    assert_eq!(read.blackline.notices, [PartNotShown]);
}

/// Only a view with block lines and none that reads gives nothing; an empty document is an
/// empty blackline.
#[test]
fn a_view_with_nothing_readable_fails() {
    let view = |text: &str| with_text(fixture!("view-nested.json"), |_| text.to_owned());
    let failure = read_view(&view("{stet 1 mode=redline}\np1 a } b\nzz")).unwrap_err();
    assert!(
        matches!(failure, ViewFailure::NothingRead { skipped: 2, .. }),
        "{failure}"
    );
    let empty = read_view(&view("{stet 1 mode=redline}\n{summary: 1 section}")).unwrap();
    assert_eq!(empty.blackline.paragraphs, []);
    assert_eq!(empty.blackline.notices, []);
}

/// A header with another format id, or none, is refused: its lines may mean something else.
#[test]
fn a_format_other_than_stet_1_is_refused() {
    let view = |text: &str| with_text(fixture!("view-nested.json"), |_| text.to_owned());
    for text in [
        "{stet 2 view=body mode=redline}\np1 a",
        "p1 a",
        "{stet10 x}\np1 a",
    ] {
        assert_eq!(
            read_view(&view(text)),
            Err(ViewFailure::FormatUnknown),
            "{text}"
        );
    }
    assert!(read_view(&view("\n{stet 1}\np1 a")).is_ok());
}

/// `depth` tables, one inside the other, each with a paragraph, and a paragraph after them.
fn nested(depth: usize) -> String {
    let opens = (1..=depth).map(|level| {
        format!("t{level} {{+}} {{table cols=1}}\nr{level} {{row}}\np{level} Level {level}.\n")
    });
    let mut text: String = opens.collect();
    text.push_str(&"{/table}\n".repeat(depth));
    text.push_str("p0 After.");
    text
}

/// Tables nest as deep as the cap; past it the too-deep table's lines are left out, and the
/// lines after it are read.
#[test]
fn tables_past_the_depth_cap_are_left_out() {
    let deepest = read_text(&nested(MAX_TABLE_DEPTH));
    assert_eq!(deepest.skipped, 0);
    assert_eq!(deepest.paragraphs.len(), MAX_TABLE_DEPTH + 1);
    let past = read_text(&nested(MAX_TABLE_DEPTH + 1));
    // The `t`, `r` and `p` lines of the one table past the cap.
    assert_eq!(past.skipped, 3);
    assert_eq!(past.paragraphs.len(), MAX_TABLE_DEPTH + 2);
    assert_eq!(past.paragraphs[MAX_TABLE_DEPTH], gap());
    assert_eq!(
        past.paragraphs.last(),
        Some(&para(Kept, &[(Same, "After.")]))
    );
}

/// A change inside a change is deleted when either is, to the cap; past it the line is left
/// out.
#[test]
fn changes_nest_to_the_cap() {
    assert_eq!(
        read_text("p1 {++a{--b--}{#2 X}c++}{#1 X}").paragraphs,
        [para(Changed, &[(Ins, "a"), (Del, "b"), (Ins, "c")])]
    );
    let nest = |depth: usize| {
        let open = "{++".repeat(depth);
        let close = "++}{#1 X}".repeat(depth);
        read_text(&format!("p1 {open}x{close}"))
    };
    assert_eq!(
        nest(MAX_CHANGE_DEPTH).paragraphs,
        [para(Changed, &[(Ins, "x")])]
    );
    let past = nest(MAX_CHANGE_DEPTH + 1);
    assert_eq!(past.paragraphs, [gap()]);
    assert_eq!(
        past.first_skip.map(|at| at.why),
        Some("changes nested past the cap")
    );
}

/// A tracked format change only (`{rev run #0 C}`): stetdoc 0.9.3's real output for a sentence
/// struck through, and for one hidden. Each paragraph is `formatted`, never `same`; the hidden
/// one says it holds hidden text, and the blackline gives the notice.
#[test]
fn a_tracked_format_change_is_a_formatted_paragraph() {
    let struck = read(fixture!("view-strike.json"));
    assert_eq!(struck.blackline.notices, []);
    assert_eq!(
        struck.blackline.paragraphs[1],
        para(Formatted, &[(Same, "Keep this. Strike this sentence.")])
    );
    let hide = read(fixture!("view-hide.json"));
    assert_eq!(hide.blackline.notices, [HiddenText]);
    let expected = para(Formatted, &[(Same, "Keep this. Hide this sentence.")]);
    assert_eq!(hide.blackline.paragraphs[1], hidden(expected));
    assert!(!hide.blackline.paragraphs[0].hidden);
}

/// stetdoc 0.9.3 counts 6 changes for a body, footnote and header that each changed; the body's
/// line names 2, so the blackline says changes outside the main text are not shown.
#[test]
fn changes_outside_the_main_text_are_noted() {
    let read = read(fixture!("view-notes.json"));
    assert_eq!(read.blackline.notices, [ChangesOutsideText]);
    assert_eq!(read.blackline.paragraphs, [term("The term is ")]);
    // A page cut short shows fewer of the body's changes too, and says that instead.
    let cut = fixture!("view-notes.json").replace("\"shown\": 1", "\"shown\": 0");
    assert_eq!(read_view(&cut).unwrap().blackline.notices, []);
}

/// A line of the view's own this reader does not know, and a table's format change, which no
/// paragraph shows, each say the blackline may differ; the rest reads as before.
#[test]
fn an_unknown_view_line_or_a_table_format_change_says_it_may_differ() {
    let whole = read(fixture!("view-nested.json")).blackline.paragraphs;
    for change in ["{footnote 1}", "{zz later line}"] {
        let view = with_text(fixture!("view-nested.json"), |text| {
            text.replace("\np6 ", &format!("\n{change}\np6 "))
        });
        let read = read_view(&view).unwrap();
        assert_eq!(read.blackline.notices, [MayDiffer], "{change}");
        assert_eq!(read.blackline.paragraphs, whole);
    }
    let view = with_text(fixture!("view-nested.json"), |text| {
        text.replace("t2 {+} {table", "t2 {+} {rev table #9 C} {table")
    });
    let read = read_view(&view).unwrap();
    assert_eq!((read.skipped, read.blackline.notices), (0, vec![MayDiffer]));
}

/// Lines joined by trailing backslashes cost time in their length, not its square: 50,000
/// physical lines of backslashes, each going on in the next, read well within a second.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test bounds the read's wall time, which only the clock can measure"
)]
fn joining_wrapped_lines_is_linear() {
    let lines = "\\\\\\\n".repeat(50_000);
    let text = format!("p1 a{lines}b");
    let started = std::time::Instant::now();
    let read = read_text(&text);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(read.paragraphs.len(), 1);
}

/// A tracked row or cell (stetdoc 0.9.3's view of documents that hold one): its paragraphs show
/// no change of their own, so each is `added` or `removed` whole, in a nested table too, and
/// the rows and cells around them are untouched.
#[test]
fn tracked_rows_and_cells_mark_their_paragraphs() {
    let paragraphs = |json: &str| {
        let read = read(json);
        assert_eq!(read.blackline.notices, [], "{json}");
        read.blackline.paragraphs
    };
    let kept = |text| para(Kept, &[(Same, text)]);
    assert_eq!(
        paragraphs(fixture!("view-row-ins.json")),
        [
            kept("Intro."),
            kept("Kept A."),
            kept("Kept B."),
            para(Added, &[(Ins, "New A.")]),
            para(Added, &[(Ins, "New B.")]),
        ]
    );
    assert_eq!(
        paragraphs(fixture!("view-row-del.json"))[3..],
        [
            para(Removed, &[(Del, "Gone A.")]),
            para(Removed, &[(Del, "Gone B.")])
        ]
    );
    assert_eq!(
        paragraphs(fixture!("view-cell-ins.json"))[1..],
        [kept("Kept A."), para(Added, &[(Ins, "New cell.")])]
    );
    assert_eq!(
        paragraphs(fixture!("view-cell-del.json"))[1..],
        [kept("Kept A."), para(Removed, &[(Del, "Gone cell.")])]
    );
    // The inserted inner row's paragraph, and nothing of the outer cell around it.
    assert_eq!(
        paragraphs(fixture!("view-row-ins-nested.json"))[1..],
        [
            kept("Outer."),
            kept("Inner kept."),
            para(Added, &[(Ins, "Inner new.")]),
            para(Kept, &[]),
        ]
    );
}

/// A cell's mark covers the later blocks of its cell (`{+}`) and ends at the next cell or row;
/// a row or cell word this reader does not know, or a row's format change, says the blackline
/// may differ.
#[test]
fn a_cell_mark_spans_its_cell_and_unknown_row_words_may_differ() {
    let read = read_text(concat!(
        "t1 {table cols=2}\n",
        "r1 {row}\n",
        "p1 {cell deleted #3 C} Gone.\n",
        "p2 {+} Also gone.\n",
        "p3 Kept.\n",
        "r2 {row}\n",
        "p4 Kept too.\n",
        "{/table}"
    ));
    let changes: Vec<_> = read.paragraphs.iter().map(|p| p.change).collect();
    assert_eq!(changes, [Removed, Removed, Kept, Kept]);
    assert!(!read.seen.unknown);
    for odd in [
        "r1 {row repeating #3 C}",
        "r1 {row} {rev row #4 C}",
        "r1 {row}\np1 {cell merged #5 C} x",
    ] {
        let read = read_text(&format!("t1 {{table cols=1}}\n{odd}\n{{/table}}"));
        assert!(read.seen.unknown, "{odd}");
    }
}

/// A summary whose count can't be read says the blackline may differ, rather than turning the
/// count check off without a word.
#[test]
fn an_unreadable_summary_may_differ() {
    let view = with_text(fixture!("view-notes.json"), |text| {
        text.replace("6 tracked changes", "six tracked changes")
    });
    assert_eq!(read_view(&view).unwrap().blackline.notices, [MayDiffer]);
}

/// A deleted row holding an inserted cell is gone in the newer version: its paragraphs are
/// removed. A deleted cell in an inserted row likewise.
#[test]
fn a_deletion_around_an_insertion_wins() {
    let changes = |text: &str| -> Vec<_> {
        read_text(text)
            .paragraphs
            .iter()
            .map(|p| p.change)
            .collect()
    };
    let deleted_row =
        "t1 {table cols=1}\nr1 {row deleted #1 C}\np1 {cell inserted #2 C} x\n{/table}";
    assert_eq!(changes(deleted_row), [Removed]);
    let inserted_row =
        "t1 {table cols=1}\nr1 {row inserted #1 C}\np1 {cell deleted #2 C} x\n{/table}";
    assert_eq!(changes(inserted_row), [Removed]);
    let nested = concat!(
        "t1 {table cols=1}\nr1 {row deleted #1 C}\np1 a\n",
        "t2 {+} {table cols=1}\nr2 {row inserted #2 C}\np2 b\n{/table}\n{/table}"
    );
    assert_eq!(changes(nested), [Removed, Removed]);
}

/// A section's tracked change, a mark or a format change (page size, margins), shows in no
/// paragraph: it says the blackline may differ. A plain section end says nothing.
#[test]
fn a_tracked_section_change_may_differ() {
    let unknown = |line: &str| read_text(&format!("p1 a\n{line}\np2 b")).seen.unknown;
    assert!(unknown("{section 1 ends: next-page rev section #4 C}"));
    assert!(unknown(
        "{section 1 ends: next-page mark inserted #3 C peer -}"
    ));
    assert!(!unknown("{section 1 ends: next-page}"));
    assert!(!unknown("{section 2 ends: continuous title-page}"));
}

/// A run that stands for an object, inserted or deleted.
fn object(kind: RunKind, object: BlacklineObject) -> BlacklineRun {
    BlacklineRun {
        kind,
        text: String::new(),
        object: Some(object),
    }
}

/// A change with no text of its own is never dropped: stetdoc 0.9.3's inserted signature
/// picture (`{++{img}++}{#0 C}`) is an inserted picture, and a deleted note's reference, a field,
/// a link and an empty deletion each stand as their object, so the paragraph is changed.
#[test]
fn a_change_with_no_text_stands_as_its_object() {
    let read = read(fixture!("view-picture.json"));
    assert_eq!(read.blackline.notices, []);
    let mut expected = para(Changed, &[(Same, "Signed: ")]);
    expected.runs.push(object(Ins, BlacklineObject::Picture));
    assert_eq!(read.blackline.paragraphs[1], expected);
    for (line, kind, what) in [
        ("p1 a{--{fn1}--}{#1 C}", Del, BlacklineObject::Note),
        ("p1 a{++{page}++}{#1 C}", Ins, BlacklineObject::Field),
        (
            "p1 a{++{chart \"Sales\"}++}{#1 C}",
            Ins,
            BlacklineObject::Chart,
        ),
        (
            "p1 a{--{link \"x\"}{/link}--}{#1 C}",
            Del,
            BlacklineObject::Other,
        ),
        ("p1 a{----}{#1 C}", Del, BlacklineObject::Other),
    ] {
        let mut expected = para(Changed, &[(Same, "a")]);
        expected.runs.push(object(kind, what));
        assert_eq!(read_text(line).paragraphs, [expected], "{line}");
    }
}

/// stetdoc marks nothing when only a link's address changes; the reader shows the view as it
/// is (the host, which has both files, says it may differ: `run.rs`).
#[test]
fn a_changed_link_address_reads_as_the_view_shows_it() {
    let read = read(fixture!("view-link.json"));
    assert_eq!(read.blackline.notices, []);
    assert!(read.blackline.paragraphs.iter().all(|p| p.change == Kept));
}

/// A view with no summary can't be checked, and a change a line names that no paragraph shows
/// as changed may be missing: each says the blackline may differ.
#[test]
fn a_missing_summary_or_an_unshown_change_may_differ() {
    let no_summary = with_text(fixture!("view-nested.json"), |text| {
        text.replace("{summary: 2 tracked changes (C 2) · 1 section}\n", "")
    });
    assert_eq!(
        read_view(&no_summary).unwrap().blackline.notices,
        [MayDiffer]
    );
    let unshown = with_text(fixture!("view-nested.json"), |text| {
        text.replace("p6 Last", "p6 {style \"Body\" #7 C} Last")
            .replace("2 tracked changes (C 2)", "3 tracked changes (C 3)")
    });
    assert_eq!(read_view(&unshown).unwrap().blackline.notices, [MayDiffer]);
}

/// The rule, over every fixture view stetdoc printed: each change it counted is shown, as a
/// changed paragraph or a placeholder, or a notice says the blackline may not show it all. The
/// reader models a closed list of tokens; anything else it meets can't pass for unchanged.
#[test]
fn every_change_stetdoc_counts_is_shown_or_noted() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/blackline");
    let mut views = vec![dir.join("view.json")];
    for entry in std::fs::read_dir(dir.join("partial")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let json = path.extension().is_some_and(|ext| ext == "json");
        if name.starts_with("view-") && json {
            views.push(path);
        }
    }
    assert!(views.len() >= 17, "{views:?}");
    for path in views {
        let json = std::fs::read_to_string(&path).unwrap();
        let notices = read_view(&json).unwrap().blackline.notices;
        let view: serde_json::Value = serde_json::from_str(&json).unwrap();
        let text = read_text(view["text"].as_str().unwrap());
        let noted = notices
            .iter()
            .any(|notice| matches!(notice, PartNotShown | ChangesOutsideText | MayDiffer));
        assert!(
            noted || text.covered.len() >= text.counted,
            "{}: {} shown of {} counted, notices {notices:?}",
            path.display(),
            text.covered.len(),
            text.counted
        );
    }
}

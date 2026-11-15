//! Every form stetdoc prints a tracked change in, read by one rule: a tag that carries
//! `#<id> <author>` names a change, which the blackline shows (the paragraph's change) or says
//! may differ; a tag the reader does not know, or an id with no author, gives "may differ". One
//! test for each form, the lines as stetdoc 0.9.3 prints them (`partial/view-props.json` holds
//! the table, grid, row, paragraph and move forms), and the words its next release adds.

use clauseo_protocol::BlacklineNotice::MayDiffer;
use clauseo_protocol::ParagraphChange::{self, Added, Changed, Formatted, Removed, Same as Kept};
use clauseo_protocol::{BlacklineNotice, RunKind};

use super::super::{Read, read_view};

/// A view whose body is `lines`, with a head and a summary that counts none (so no count
/// check fires).
fn view(lines: &str) -> String {
    let text = format!(
        "{{stet 1 view=body mode=redline}}\n{{authors: C \"Clauseo\"}}\n\
         {{summary: 1 section}}\n{lines}\n"
    );
    serde_json::json!({
        "mode": "redline",
        "text": text,
        "next": null,
        "position": {"shown": 1, "total": 1},
        "warnings": [],
    })
    .to_string()
}

/// `lines` read.
fn read(lines: &str) -> Read {
    read_view(&view(lines)).unwrap()
}

/// The ids `lines` name, the paragraphs' changes and the notices.
fn seen(lines: &str) -> (Vec<u64>, Vec<ParagraphChange>, Vec<BlacklineNotice>) {
    let read = read(lines);
    let changes = read.blackline.paragraphs.iter().map(|p| p.change).collect();
    (
        read.ids.into_iter().collect(),
        changes,
        read.blackline.notices,
    )
}

/// A one-cell table around `cell`, its row marked `row`.
fn table(row: &str, cell: &str) -> String {
    format!("t1 {{table cols=1}}\nr1 {row}\n{cell}\n{{/table}}")
}

#[test]
fn rev_run() {
    assert_eq!(
        seen("p1 {rev run #4 C} Struck."),
        (vec![4], vec![Formatted], vec![])
    );
}

#[test]
fn rev_para() {
    assert_eq!(
        seen("p1 {§\"1.\"} {rev para #25 R} Numbered now."),
        (vec![25], vec![Formatted], vec![])
    );
}

#[test]
fn rev_numbering() {
    assert_eq!(
        seen("p1 {rev numbering #4 C} Renumbered."),
        (vec![4], vec![Formatted], vec![])
    );
}

/// A table's, its grid's and a row's format change: no paragraph shows them, so may differ.
#[test]
fn rev_table() {
    let lines = "t1 {table cols=2} {rev table #23 R}\nr1 {row}\np1 A\n{/table}";
    assert_eq!(seen(lines), (vec![23], vec![Kept], vec![MayDiffer]));
    let before = "t1 {rev table #23 R} {table cols=2}\nr1 {row}\np1 A\n{/table}";
    assert_eq!(seen(before), (vec![23], vec![Kept], vec![MayDiffer]));
}

#[test]
fn rev_grid() {
    let lines = "t1 {table cols=2} {rev grid #24 R}\nr1 {row}\np1 A\n{/table}";
    assert_eq!(seen(lines), (vec![24], vec![Kept], vec![MayDiffer]));
}

#[test]
fn rev_row() {
    assert_eq!(
        seen(&table("{row} {rev row #22 R}", "p1 A")),
        (vec![22], vec![Kept], vec![MayDiffer])
    );
}

#[test]
fn rev_section() {
    let lines = "p1 Text.\n{section 1 ends: next-page rev section #4 C}";
    assert_eq!(seen(lines), (vec![4], vec![Kept], vec![MayDiffer]));
}

#[test]
fn row_inserted() {
    assert_eq!(
        seen(&table("{row inserted #11 R}", "p1 New.")),
        (vec![11], vec![Added], vec![])
    );
}

#[test]
fn cell_inserted() {
    assert_eq!(
        seen(&table("{row}", "p1 {cell inserted #13 R} New cell.")),
        (vec![13], vec![Added], vec![])
    );
}

#[test]
fn paragraph_mark_inserted() {
    assert_eq!(
        seen("p1 {¶+ #5 C} {++Notices go by email.++}{#4 C}"),
        (vec![4, 5], vec![Added], vec![])
    );
}

#[test]
fn paragraph_mark_deleted() {
    assert_eq!(
        seen("p1 {¶- #3 C} {--The deposit is returned.--}{#2 C}"),
        (vec![2, 3], vec![Removed], vec![])
    );
}

#[test]
fn insertion() {
    let read = read("p1 Rent of $1,{++200++}{#6 C}.");
    assert_eq!(read.ids.into_iter().collect::<Vec<_>>(), [6]);
    let runs = &read.blackline.paragraphs[0].runs;
    assert_eq!(runs[1].kind, RunKind::Ins);
    assert_eq!(read.blackline.paragraphs[0].change, Changed);
    assert_eq!(read.blackline.notices, []);
}

#[test]
fn deletion() {
    let read = read("p1 Rent of $1,{--000--}{#7 C}.");
    assert_eq!(read.ids.into_iter().collect::<Vec<_>>(), [7]);
    assert_eq!(read.blackline.paragraphs[0].runs[1].kind, RunKind::Del);
    assert_eq!(read.blackline.notices, []);
}

#[test]
fn move_to() {
    assert_eq!(
        seen("p1 {--Moved text.--}{#31 R move to -}"),
        (vec![31], vec![Changed], vec![])
    );
}

#[test]
fn move_from() {
    assert_eq!(
        seen("p1 {++Moved text.++}{#33 R move from -}"),
        (vec![33], vec![Changed], vec![])
    );
}

/// stetdoc's next release: a field's `label` word and a repeating section's `item=N` row, which
/// name no change unless they carry `#<id> <author>`.
#[test]
fn a_label_word() {
    let read = read("p1 Paid under clause {field REF \"4.2\" ->p57 label}.");
    assert_eq!(
        read.blackline.paragraphs[0].runs[0].text,
        "Paid under clause 4.2."
    );
    assert_eq!((read.ids.len(), read.blackline.notices), (0, vec![]));
    // The same tag with a change's id and author: a change no paragraph shows, so may differ.
    assert_eq!(
        seen("p1 Clause {field REF \"4.2\" ->p57 label #6 C}."),
        (vec![6], vec![Kept], vec![MayDiffer])
    );
}

#[test]
fn a_row_item() {
    assert_eq!(
        seen(&table("{row item=2}", "p1 A")),
        (vec![], vec![Kept], vec![])
    );
    assert_eq!(
        seen(&table("{row inserted item=2 #11 R}", "p1 A")),
        (vec![11], vec![Added], vec![])
    );
}

/// A tag the reader does not know, with an id or without, and an id with no author.
#[test]
fn an_unknown_tag_or_an_id_without_its_author_may_differ() {
    assert_eq!(
        seen("p1 {zork 3} Text."),
        (vec![], vec![Kept], vec![MayDiffer])
    );
    assert_eq!(
        seen("p1 {zork #4 C} Text."),
        (vec![4], vec![Kept], vec![MayDiffer])
    );
    assert_eq!(
        seen("p1 {rev run #4} Text."),
        (vec![4], vec![Formatted], vec![MayDiffer])
    );
    assert_eq!(
        seen(&table("{row zork}", "p1 A")),
        (vec![], vec![Kept], vec![MayDiffer])
    );
}

/// The real view of a document holding the table, grid, row, cell, paragraph, section and move
/// forms: each one the view prints is named by its id.
#[test]
fn the_real_forms_are_named_by_id() {
    let json = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/blackline/partial/view-props.json"
    ));
    let read = read_view(json).unwrap();
    assert_eq!(
        read.ids.into_iter().collect::<Vec<_>>(),
        [22, 23, 24, 25, 31, 33]
    );
}

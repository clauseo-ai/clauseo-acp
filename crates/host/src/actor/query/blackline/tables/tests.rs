//! The tables' formatting compared on .docx files built here: one change of each group, noise
//! that is no change, tables inserted beside a change, tables that can't be paired, tracked
//! deletions, `mc:Fallback`, the Strict namespace, the alignment's cap, and a file that does not
//! read. The headers, footers and notes are in `parts.rs`.

use std::io::{Cursor, Write as _};

use clauseo_protocol::TableFormat::{
    Alignment, Borders, MergedCells, Other, RowHeight, Shading, Widths,
};
use clauseo_protocol::{ChangedTable, TableFormat, TablePlace};

use super::{TablesCompared, align, compare};

pub(super) const W: &str = concat!(
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" "#,
    r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#
);

pub(super) const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A .docx whose body is `body`, with `parts` (name, XML) and the main part's relationships
/// `rels`.
pub(super) fn docx_with(body: &str, parts: &[(&str, String)], rels: &str) -> Vec<u8> {
    let document = format!("<w:document {W}><w:body>{body}</w:body></w:document>");
    docx_of(&document, parts, rels)
}

/// A .docx whose main part is `document`, with `parts` (name, XML) and the main part's
/// relationships `rels`.
pub(super) fn docx_of(document: &str, parts: &[(&str, String)], rels: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let package_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>"#
    );
    let document_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
    );
    let mut all = vec![
        ("_rels/.rels", package_rels),
        ("word/_rels/document.xml.rels", document_rels),
        ("word/document.xml", document.to_owned()),
    ];
    all.extend(parts.iter().map(|(name, xml)| (*name, xml.clone())));
    for (name, text) in all {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// A .docx whose body is `body`.
fn docx(body: &str) -> Vec<u8> {
    docx_with(body, &[], "")
}

/// A paragraph of `text`.
pub(super) fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}

/// A cell holding `text`, with `tcpr` in its properties.
pub(super) fn cell(text: &str, tcpr: &str) -> String {
    format!(
        r#"<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/>{tcpr}</w:tcPr>{}</w:tc>"#,
        p(text)
    )
}

/// A row of `cells`, with `trpr` in its properties.
pub(super) fn row(cells: &str, trpr: &str) -> String {
    format!("<w:tr><w:trPr>{trpr}</w:trPr>{cells}</w:tr>")
}

/// A table of `rows`, with `tblpr` in its properties and the grid `cols`.
pub(super) fn table(rows: &str, tblpr: &str, cols: &str) -> String {
    format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/>{tblpr}</w:tblPr><w:tblGrid>{cols}</w:tblGrid>{rows}</w:tbl>"#
    )
}

/// Two columns of 2000.
pub(super) const COLS: &str = r#"<w:gridCol w:w="2000"/><w:gridCol w:w="2000"/>"#;

/// A 2x2 table named `name`, with `tblpr`, `trpr` on its first row and `tcpr` on its last
/// cell.
pub(super) fn grid(name: &str, tblpr: &str, trpr: &str, tcpr: &str) -> String {
    let first = row(&(cell(name, "") + &cell("A", "")), trpr);
    let second = row(&(cell("B", "") + &cell("C", tcpr)), "");
    table(&(first + &second), tblpr, COLS)
}

/// The body's table `number` changed by `formats`.
pub(super) fn body(number: u32, formats: &[TableFormat]) -> ChangedTable {
    ChangedTable {
        place: TablePlace::Body,
        number,
        formats: formats.to_vec(),
    }
}

/// `compare` on two bodies.
fn bodies(old: &str, new: &str) -> TablesCompared {
    compare(&docx(old), &docx(new))
}

/// One change of each group, in a table's own properties, its grid, a row's and a cell's.
#[test]
fn each_group_of_formatting_is_named() {
    let plain = grid("T", "", "", "");
    let cases: [(String, TableFormat); 9] = [
        (
            grid(
                "T",
                "",
                "",
                r#"<w:tcBorders><w:top w:val="single"/></w:tcBorders>"#,
            ),
            Borders,
        ),
        (
            grid("T", "", "", r#"<w:shd w:val="clear" w:fill="FFFF00"/>"#),
            Shading,
        ),
        (
            table(
                &(row(&(cell("T", "") + &cell("A", "")), "")
                    + &row(&(cell("B", "") + &cell("C", "")), "")),
                "",
                r#"<w:gridCol w:w="3000"/><w:gridCol w:w="1000"/>"#,
            ),
            Widths,
        ),
        (grid("T", r#"<w:jc w:val="center"/>"#, "", ""), Alignment),
        (
            grid("T", "", "", r#"<w:vAlign w:val="bottom"/>"#),
            Alignment,
        ),
        (
            grid("T", "", "", r#"<w:vMerge w:val="restart"/>"#),
            MergedCells,
        ),
        (grid("T", "", r#"<w:trHeight w:val="720"/>"#, ""), RowHeight),
        (grid("T", r#"<w:tblStyle w:val="Grid"/>"#, "", ""), Other),
        (grid("T", "", "<w:cantSplit/>", ""), Other),
    ];
    for (changed, format) in cases {
        assert_eq!(
            bodies(&plain, &changed),
            TablesCompared {
                changed: vec![body(1, &[format])],
                may_differ: false,
                ..TablesCompared::default()
            },
            "{format:?}"
        );
    }
}

/// rsid attributes, Word's paragraph ids, a tracked change's old properties and the tracked
/// marks are no change; and the same file is none.
#[test]
fn noise_alone_is_no_change() {
    let plain = grid("T", "", "", "");
    let noisy = grid(
        "T",
        r#"<w:tblPrChange w:id="1" w:author="A"><w:tblPr><w:jc w:val="left"/></w:tblPr></w:tblPrChange>"#,
        r#"<w:ins w:id="2" w:author="A"/>"#,
        r#"<w:tcPrChange w:id="3" w:author="A"><w:tcPr><w:shd w:fill="00FF00"/></w:tcPr></w:tcPrChange>"#,
    )
    .replace("<w:tr>", r#"<w:tr w:rsidR="00AB12CD" w14:paraId="1A2B3C4D" w14:textId="77777777">"#)
    .replace("<w:tcW w:w", r#"<w:tcW w:rsidR="00FF0000" w:w"#);
    assert_eq!(bodies(&plain, &noisy), TablesCompared::default());
    assert_eq!(bodies(&plain, &plain), TablesCompared::default());
}

/// A tracked change's old grid (`w:tblGridChange`) and old table exceptions
/// (`w:tblPrExChange`), and the style marks Word works out from a row's place
/// (`w:cnfStyle`), are no change.
#[test]
fn old_grids_old_exceptions_and_style_marks_are_no_change() {
    let plain = grid("T", "", "", "");
    let marks = r#"<w:cnfStyle w:val="100000000000"/>"#;
    let noisy = grid("T", "", marks, marks)
        .replace(
            "</w:tblGrid>",
            r#"<w:tblGridChange w:id="4" w:author="A"><w:tblGrid><w:gridCol w:w="9000"/></w:tblGrid></w:tblGridChange></w:tblGrid>"#,
        )
        .replacen(
            "<w:tr>",
            r#"<w:tr><w:tblPrEx><w:tblPrExChange w:id="5" w:author="A"><w:tblPrEx><w:tblBorders><w:top w:val="single"/></w:tblBorders></w:tblPrEx></w:tblPrExChange></w:tblPrEx>"#,
            1,
        );
    assert_eq!(bodies(&plain, &noisy), TablesCompared::default());
}

/// A table whose every row's deletion is tracked is gone, as accepting them leaves it: it takes
/// no number, and the tables after it pair by order.
#[test]
fn a_table_whose_rows_are_all_deleted_is_left_out() {
    let deleted = r#"<w:del w:id="6" w:author="A"/>"#;
    let gone = table(
        &row(&(cell("Gone", "") + &cell("Too", "")), deleted),
        "",
        COLS,
    );
    let shaded = r#"<w:shd w:val="clear" w:fill="FFFF00"/>"#;
    let old = grid("X", "", "", "") + &grid("Y", "", "", "");
    let new = gone + &grid("X", "", "", "") + &grid("Y", "", "", shaded);
    assert_eq!(
        bodies(&old, &new),
        TablesCompared {
            changed: vec![body(2, &[Shading])],
            ..TablesCompared::default()
        }
    );
}

/// A table inserted before one whose shading changed: the tables pair by their text, and the
/// changed one is named by its number in the newer version. Nothing is unpaired on both sides.
#[test]
fn a_table_inserted_beside_a_change_pairs_by_text() {
    let old = grid("First", "", "", "") + &p("x") + &grid("Second", "", "", "");
    let shaded = r#"<w:shd w:val="clear" w:fill="FFFF00"/>"#;
    let new = grid("New", "", "", "")
        + &grid("First", "", "", "")
        + &p("x")
        + &grid("Second", "", "", shaded);
    assert_eq!(
        bodies(&old, &new),
        TablesCompared {
            changed: vec![body(3, &[Shading])],
            may_differ: false,
            ..TablesCompared::default()
        }
    );
}

/// Two tables whose text changed become three: none can be paired, so what changed in them
/// can't be put on a table, and the blackline may differ.
#[test]
fn tables_that_can_not_be_paired_may_differ() {
    let old = grid("One", "", "", "") + &grid("Two", "", "", "");
    let new = grid("Uno", "", "", "") + &grid("Dos", "", "", "") + &grid("Tres", "", "", "");
    assert_eq!(
        bodies(&old, &new),
        TablesCompared {
            changed: Vec::new(),
            may_differ: true,
            ..TablesCompared::default()
        }
    );
    // A row of the table split in two with new text: the rows can't be paired either.
    let split = table(
        &(row(&(cell("T", "") + &cell("A", "")), "")
            + &row(&(cell("B1", "") + &cell("C1", "")), "")
            + &row(&(cell("B2", "") + &cell("C2", "")), "")),
        "",
        COLS,
    );
    assert!(bodies(&grid("T", "", "", ""), &split).may_differ);
}

/// A table inside a cell counts in the order it starts; its change is its own.
#[test]
fn a_table_inside_a_cell_is_numbered_in_order() {
    let inner = |tcpr: &str| table(&row(&cell("Inner", tcpr), ""), "", COLS);
    let outer = |tcpr: &str| {
        let holder = format!("<w:tc><w:tcPr/>{}{}</w:tc>", inner(tcpr), p(""));
        table(&row(&(holder + &cell("Outer", "")), ""), "", COLS)
    };
    let shaded = r#"<w:shd w:val="clear" w:fill="FFFF00"/>"#;
    assert_eq!(
        bodies(&outer(""), &outer(shaded)).changed,
        [body(2, &[Shading])]
    );
}

/// Every changed table is listed, uncapped (the blackline caps them, `run.rs`); a file that
/// does not read may differ.
#[test]
fn many_tables_are_all_listed_and_unreadable_files_may_differ() {
    let shaded = r#"<w:shd w:val="clear" w:fill="FFFF00"/>"#;
    let many = |tcpr: &str| {
        (0..60)
            .map(|n| grid(&format!("T{n}"), "", "", tcpr))
            .collect::<String>()
    };
    let found = bodies(&many(""), &many(shaded));
    assert_eq!((found.changed.len(), found.may_differ), (60, false));
    let plain = docx(&grid("T", "", "", ""));
    for broken in [b"not a zip".to_vec(), docx("<w:tbl><w:tr>")] {
        assert!(compare(&plain, &broken).may_differ);
        assert!(compare(&broken, &plain).may_differ);
    }
}

/// The pairing on its own: by order when as many; by text, then by order between pairs.
#[test]
fn align_pairs_by_order_then_by_text() {
    assert_eq!(
        align(&["a", "b"], &["x", "y"]),
        (vec![(0, 0), (1, 1)], false)
    );
    assert_eq!(
        align(&["a", "b"], &["n", "a", "b"]),
        (vec![(0, 1), (1, 2)], false)
    );
    assert_eq!(
        align(&["a", "b", "c"], &["a", "B", "c", "d"]),
        (vec![(0, 0), (1, 1), (2, 2)], false)
    );
    assert_eq!(
        align(&["a", "b", "c"], &["a", "x", "y", "c"]),
        (vec![(0, 0), (2, 3)], true)
    );
    assert_eq!(align(&[], &["a"]), (vec![], false));
}

/// A shaded cell's tcPr.
pub(super) const SHADED: &str = r#"<w:shd w:val="clear" w:fill="FFFF00"/>"#;

/// A row whose deletion is tracked, its cells shaded, is left out: it pairs with nothing, so the
/// copy of the row below it, unshaded, pairs with the older row.
#[test]
fn a_row_whose_deletion_is_tracked_is_left_out() {
    let deleted = r#"<w:del w:id="7" w:author="A"/>"#;
    let rows = |extra: &str| {
        row(&(cell("A", "") + &cell("1", "")), "")
            + extra
            + &row(&(cell("B", "") + &cell("2", "")), "")
    };
    let old = table(&rows(""), "", COLS);
    let gone = row(&(cell("B", SHADED) + &cell("2", SHADED)), deleted);
    let new = table(&rows(&gone), "", COLS);
    assert_eq!(bodies(&old, &new), TablesCompared::default());
}

/// A cell whose deletion is tracked, shaded, is left out in the same way.
#[test]
fn a_cell_whose_deletion_is_tracked_is_left_out() {
    let deleted = r#"<w:cellDel w:id="8" w:author="A"/>"#;
    let old = table(&row(&(cell("A", "") + &cell("B", "")), ""), "", COLS);
    let cells = cell("A", "") + &cell("B", &format!("{SHADED}{deleted}")) + &cell("B", "");
    let new = table(&row(&cells, ""), "", COLS);
    assert_eq!(bodies(&old, &new), TablesCompared::default());
}

/// A table that `mc:Fallback` repeats is read once, from `mc:Choice`'s side: the shaded table is
/// the newer version's only one.
#[test]
fn a_table_mc_fallback_repeats_is_read_once() {
    let fallback = format!(
        r#"<mc:AlternateContent><mc:Choice Requires="w14"/><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>"#,
        grid("T", "", "", "")
    );
    let new = fallback + &grid("T", "", "", SHADED);
    assert_eq!(
        bodies(&grid("T", "", "", ""), &new).changed,
        [body(1, &[Shading])]
    );
}

/// A file in Word's Strict namespace is read as a Transitional one is.
#[test]
fn a_strict_file_is_read() {
    let strict = |tcpr: &str| {
        let document = format!(
            r#"<w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body>{}</w:body></w:document>"#,
            grid("T", "", "", tcpr)
        );
        docx_of(&document, &[], "")
    };
    assert_eq!(
        compare(&strict(""), &strict(SHADED)).changed,
        [body(1, &[Shading])]
    );
}

/// Cells left unpaired on both sides of a paired row (a row whose cells changed shape and text)
/// may differ.
#[test]
fn cells_unpaired_on_both_sides_may_differ() {
    let old = table(
        &row(&(cell("A", "") + &cell("B", "") + &cell("C", "")), ""),
        "",
        COLS,
    );
    let new = table(&row(&(cell("X", "") + &cell("Y", "")), ""), "", COLS);
    assert!(bodies(&old, &new).may_differ);
}

/// Past [`super::ALIGN_MAX`] cells, a list is not aligned, even where its texts would pair: what
/// is left unpaired may differ.
#[test]
fn a_list_past_the_alignment_cap_may_differ() {
    let old = vec!["a"; 2001];
    let new = vec!["a"; 2002];
    assert!(old.len() * new.len() > super::ALIGN_MAX);
    assert_eq!(align(&old, &new), (vec![], true));
}

mod parts;

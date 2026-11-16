//! The tables' formatting compared on .docx files built here: one change of each group, noise
//! that is no change, tables inserted beside a change, tables that can't be paired, a header's
//! table, and a file that does not read.

use std::io::{Cursor, Write as _};

use clauseo_protocol::TableFormat::{
    Alignment, Borders, MergedCells, Other, RowHeight, Shading, Widths,
};
use clauseo_protocol::{ChangedTable, HeaderSlot, HeaderUse, TableFormat, TablePlace};

use super::{TABLES_MAX, TablesCompared, align, compare};

const W: &str = concat!(
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml""#
);

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A .docx whose body is `body`, with `parts` (name, XML) and the main part's relationships
/// `rels`.
fn docx_with(body: &str, parts: &[(&str, String)], rels: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let package_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>"#
    );
    let document_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
    );
    let document = format!("<w:document {W}><w:body>{body}</w:body></w:document>");
    let mut all = vec![
        ("_rels/.rels", package_rels),
        ("word/_rels/document.xml.rels", document_rels),
        ("word/document.xml", document),
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
fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}

/// A cell holding `text`, with `tcpr` in its properties.
fn cell(text: &str, tcpr: &str) -> String {
    format!(
        r#"<w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/>{tcpr}</w:tcPr>{}</w:tc>"#,
        p(text)
    )
}

/// A row of `cells`, with `trpr` in its properties.
fn row(cells: &str, trpr: &str) -> String {
    format!("<w:tr><w:trPr>{trpr}</w:trPr>{cells}</w:tr>")
}

/// A table of `rows`, with `tblpr` in its properties and the grid `cols`.
fn table(rows: &str, tblpr: &str, cols: &str) -> String {
    format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/>{tblpr}</w:tblPr><w:tblGrid>{cols}</w:tblGrid>{rows}</w:tbl>"#
    )
}

/// Two columns of 2000.
const COLS: &str = r#"<w:gridCol w:w="2000"/><w:gridCol w:w="2000"/>"#;

/// A 2x2 table named `name`, with `tblpr`, `trpr` on its first row and `tcpr` on its last
/// cell.
fn grid(name: &str, tblpr: &str, trpr: &str, tcpr: &str) -> String {
    let first = row(&(cell(name, "") + &cell("A", "")), trpr);
    let second = row(&(cell("B", "") + &cell("C", tcpr)), "");
    table(&(first + &second), tblpr, COLS)
}

/// The body's table `number` changed by `formats`.
fn body(number: u32, formats: &[TableFormat]) -> ChangedTable {
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

/// A header's table, its header found by the section's reference.
#[test]
fn a_tables_change_in_a_header_names_the_header() {
    let header = |tcpr: &str| {
        format!(
            "<w:hdr {W}>{}</w:hdr>",
            table(&row(&cell("Logo", tcpr), ""), "", COLS)
        )
    };
    let rels = format!(r#"<Relationship Id="rIdH" Type="{REL}/header" Target="header1.xml"/>"#);
    let body_xml = format!(
        r#"{}<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/></w:sectPr>"#,
        p("Body")
    );
    let file = |tcpr: &str| docx_with(&body_xml, &[("word/header1.xml", header(tcpr))], &rels);
    let found = compare(&file(""), &file(r#"<w:vAlign w:val="center"/>"#));
    let uses = vec![HeaderUse {
        slot: HeaderSlot::Default,
        sections: vec![1],
    }];
    assert_eq!(
        found,
        TablesCompared {
            changed: vec![ChangedTable {
                place: TablePlace::Header { uses },
                number: 1,
                formats: vec![Alignment],
            }],
            may_differ: false,
        }
    );
}

/// A header part holding `xml`'s body.
fn hdr(xml: &str) -> String {
    format!("<w:hdr {W}>{xml}{}</w:hdr>", p(""))
}

/// A one-cell table reading `text`, its cell with `tcpr`.
fn logo(text: &str, tcpr: &str) -> String {
    table(&row(&cell(text, tcpr), ""), "", COLS)
}

/// A file whose sections each use one header, by default: `headers` gives each section's header
/// part's XML, first to last, and the body holds `body` in the last section.
fn sectioned(headers: &[String], body: &str) -> Vec<u8> {
    let mut parts = Vec::new();
    let mut rels = String::new();
    let mut xml = String::new();
    for (index, header) in headers.iter().enumerate() {
        let name = format!("header{index}.xml");
        rels.push_str(&format!(
            r#"<Relationship Id="rIdH{index}" Type="{REL}/header" Target="{name}"/>"#
        ));
        parts.push((format!("word/{name}"), header.clone()));
        let reference = format!(r#"<w:headerReference w:type="default" r:id="rIdH{index}"/>"#);
        if index + 1 < headers.len() {
            xml.push_str(&format!(
                "<w:p><w:pPr><w:sectPr>{reference}</w:sectPr></w:pPr></w:p>"
            ));
        } else {
            xml.push_str(&format!("{}<w:sectPr>{reference}</w:sectPr>", p(body)));
        }
    }
    let parts: Vec<(&str, String)> = parts
        .iter()
        .map(|(name, xml)| (name.as_str(), xml.clone()))
        .collect();
    docx_with(&xml, &parts, &rels)
}

/// A cover section inserted before the only one, with its own header, while the shading of the
/// table in the old section's header changes: the sections pair from the last one back, as
/// stetdoc's compare pairs them, so the header with the table pairs with itself and its table
/// is named on section 2.
#[test]
fn a_header_whose_section_moved_pairs_from_the_last_section_back() {
    let shaded = r#"<w:shd w:val="clear" w:fill="FFFF00"/>"#;
    let old = sectioned(&[hdr(&logo("Logo", ""))], "One year.");
    let new = sectioned(
        &[hdr(&p("Cover header.")), hdr(&logo("Logo", shaded))],
        "Two years.",
    );
    let uses = vec![HeaderUse {
        slot: HeaderSlot::Default,
        sections: vec![2],
    }];
    assert_eq!(
        compare(&old, &new),
        TablesCompared {
            changed: vec![ChangedTable {
                place: TablePlace::Header { uses },
                number: 1,
                formats: vec![Shading],
            }],
            ..TablesCompared::default()
        }
    );
}

/// A header with a table that pairs with no header of the other version: what changed in its
/// table can't be told, so the blackline may differ.
#[test]
fn a_header_with_tables_and_no_counterpart_may_differ() {
    let old = sectioned(&[hdr(&logo("Logo", ""))], "One year.");
    let new = sectioned(
        &[hdr(&logo("Logo", "")), hdr(&logo("Seal", ""))],
        "Two years.",
    );
    assert!(compare(&old, &new).may_differ);
    // Without a table, a header with no counterpart has nothing to compare.
    let plain = sectioned(&[hdr(&p("Cover.")), hdr(&logo("Logo", ""))], "Two years.");
    assert!(!compare(&old, &plain).may_differ);
}

/// Past the cap the rest may differ; a file that does not read may differ.
#[test]
fn too_many_tables_and_unreadable_files_may_differ() {
    let shaded = r#"<w:shd w:val="clear" w:fill="FFFF00"/>"#;
    let many = |tcpr: &str| {
        (0..=TABLES_MAX)
            .map(|n| grid(&format!("T{n}"), "", "", tcpr))
            .collect::<String>()
    };
    let found = bodies(&many(""), &many(shaded));
    assert_eq!((found.changed.len(), found.may_differ), (TABLES_MAX, true));
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

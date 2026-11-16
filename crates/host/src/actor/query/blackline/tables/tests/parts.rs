//! The tables of headers, footers and notes compared: a header's table, headers paired from the
//! last section back, a header with tables and no counterpart, the header texts one version only
//! has, the notes' place, and where a header is, as stetdoc's view says and as the table
//! comparison says.

use std::fmt::Write as _;

use clauseo_protocol::TableFormat::{Alignment, Shading};
use clauseo_protocol::{BlacklinePartKind, ChangedTable, HeaderSlot, HeaderUse, TablePlace};

use super::super::read::{Where, read_package};
use super::super::{TablesCompared, compare};
use super::{COLS, REL, SHADED, W, body, cell, docx_with, grid, p, row, table};
use crate::actor::query::blackline::parse::story::{Story, read_story};

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
            ..TablesCompared::default()
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
        let _infallible = write!(
            rels,
            r#"<Relationship Id="rIdH{index}" Type="{REL}/header" Target="{name}"/>"#
        );
        parts.push((format!("word/{name}"), header.clone()));
        let reference = format!(r#"<w:headerReference w:type="default" r:id="rIdH{index}"/>"#);
        if index + 1 < headers.len() {
            let _infallible = write!(
                xml,
                "<w:p><w:pPr><w:sectPr>{reference}</w:sectPr></w:pPr></w:p>"
            );
        } else {
            let _infallible = write!(xml, "{}<w:sectPr>{reference}</w:sectPr>", p(body));
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
            may_differ: false,
            lone_header_texts: vec!["Coverheader.".to_owned()],
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

/// A header's text one version only has is listed, white space taken out; a header edited in
/// place lists its text on both sides, and the same header lists nothing.
#[test]
fn a_header_text_one_version_only_has_is_listed() {
    let old = sectioned(&[hdr(&p("Main header."))], "One year.");
    let added = sectioned(
        &[hdr(&p("Schedule header.")), hdr(&p("Main header."))],
        "Two years.",
    );
    assert_eq!(compare(&old, &added).lone_header_texts, ["Scheduleheader."]);
    let edited = sectioned(&[hdr(&p("Main header, revised."))], "One year.");
    assert_eq!(
        compare(&old, &edited).lone_header_texts,
        ["Mainheader,revised.", "Mainheader."]
    );
    let blank = sectioned(&[hdr(&p("Main header.")), hdr("")], "Two years.");
    assert_eq!(
        compare(&old, &blank).lone_header_texts,
        Vec::<String>::new()
    );
}

/// A header's text reads entity and character references as the characters they stand for, and
/// a symbol (`w:sym`) as its character, as the views write them.
#[test]
fn a_header_text_reads_references_and_symbols_as_their_characters() {
    let entity = |d: &str| {
        hdr(&format!(
            "<w:p><w:r><w:t>Smith &amp; Co &#8212; Draft {d}</w:t></w:r></w:p>"
        ))
    };
    let old = sectioned(&[entity("1")], "Body.");
    let new = sectioned(&[entity("2")], "Body.");
    assert_eq!(
        compare(&old, &new).lone_header_texts,
        ["Smith&Co\u{2014}Draft1", "Smith&Co\u{2014}Draft2"]
    );
    let symbol = |d: &str| {
        hdr(&format!(
            r#"<w:p><w:r><w:sym w:font="Wingdings" w:char="F0FC"/><w:t>Draft {d}</w:t></w:r></w:p>"#
        ))
    };
    let old = sectioned(&[symbol("1")], "Body.");
    let new = sectioned(&[symbol("2")], "Body.");
    assert_eq!(
        compare(&old, &new).lone_header_texts,
        ["\u{f0fc}Draft1", "\u{f0fc}Draft2"]
    );
}

/// A table in a footnote, and one in an endnote, are named by the notes' place, after the body.
#[test]
fn a_tables_change_in_the_notes_names_the_notes() {
    let notes = |kind: &str, tcpr: &str| {
        format!(
            r#"<w:{kind}s {W}><w:{kind} w:id="1">{}</w:{kind}></w:{kind}s>"#,
            logo("Note", tcpr)
        )
    };
    let rels = format!(
        r#"<Relationship Id="rIdN" Type="{REL}/footnotes" Target="footnotes.xml"/><Relationship Id="rIdE" Type="{REL}/endnotes" Target="endnotes.xml"/>"#
    );
    let file = |tcpr: &str| {
        let parts = [
            ("word/footnotes.xml", notes("footnote", tcpr)),
            ("word/endnotes.xml", notes("endnote", tcpr)),
        ];
        docx_with(&grid("T", "", "", tcpr), &parts, &rels)
    };
    let in_notes = |place| ChangedTable {
        place,
        number: 1,
        formats: vec![Shading],
    };
    assert_eq!(
        compare(&file(""), &file(SHADED)).changed,
        [
            body(1, &[Shading]),
            in_notes(TablePlace::Footnotes),
            in_notes(TablePlace::Endnotes),
        ]
    );
}

/// Where each header and footer of the two-section pair is: the parts its headers' view shows
/// (stetdoc's `{header default sections=1,2}`, `{footer first sections=2}`) and the places the
/// table comparison works out from the sections' references agree.
#[test]
fn a_parts_uses_and_the_table_comparisons_agree() {
    let file = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/blackline/partial/stories-new.docx"
    ));
    let view = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/blackline/partial/story-headers-stories.json"
    ));
    let package = read_package(file).unwrap();
    let ours: Vec<Where> = package
        .headers
        .values()
        .map(|part| part.place.clone())
        .collect();
    let stetdocs: Vec<Where> = read_story(view, Story::Headers)
        .unwrap()
        .parts
        .into_iter()
        .map(|part| match part.kind {
            BlacklinePartKind::Header { uses } => Where::Header(uses),
            BlacklinePartKind::Footer { uses } => Where::Footer(uses),
            other @ (BlacklinePartKind::Footnote { .. }
            | BlacklinePartKind::Endnote { .. }
            | BlacklinePartKind::Unknown { .. }) => panic!("not a header or footer: {other:?}"),
        })
        .collect();
    assert_eq!(stetdocs.len(), 2);
    assert_eq!(ours.len(), stetdocs.len());
    assert!(
        stetdocs.iter().all(|place| ours.contains(place)),
        "{ours:?}"
    );
}

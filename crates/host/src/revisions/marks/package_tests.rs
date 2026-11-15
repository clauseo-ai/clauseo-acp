//! The package as the marks scan reads it: every XML part, whatever the relationships say; the
//! relationships and content types read as XML, only to notice a part they name that is missing;
//! Word's Strict format; and the cases a byte reading of either got wrong.

use std::fmt::Write as _;

use super::tests::{TYPES, declared, docx};
use super::{CHANGE_MARKS, Found, holds_any, zip_has_marks};

const STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const STRICT_RELS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// A Word file in the Strict format, as Word's "Strict Open XML Document" writes one: `w` bound
/// to the Strict namespace, Strict relationship types, with `body` in the document and `header`
/// in its one header.
fn strict(body: &str, header: &str) -> std::io::Cursor<Vec<u8>> {
    let package = format!(
        r#"<Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{STRICT_RELS}/officeDocument" Target="word/document.xml"/></Relationships>"#
    );
    let document_rels = format!(
        r#"<Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{STRICT_RELS}/header" Target="header1.xml"/></Relationships>"#
    );
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{STRICT}" xmlns:r="{STRICT_RELS}" w:conformance="strict"><w:body>{body}<w:sectPr><w:headerReference w:type="default" r:id="rId1"/></w:sectPr></w:body></w:document>"#
    );
    let header = format!(r#"<w:hdr xmlns:w="{STRICT}"><w:p>{header}</w:p></w:hdr>"#);
    docx(&[
        ("_rels/.rels", package.as_str()),
        ("word/document.xml", document.as_str()),
        ("word/_rels/document.xml.rels", document_rels.as_str()),
        ("word/header1.xml", header.as_str()),
    ])
}

#[test]
fn a_strict_word_file_has_its_changes_found() {
    let ins = r#"<w:ins w:id="1" w:author="R" w:date="2026-11-01T00:00:00Z"><w:r><w:t>new</w:t></w:r></w:ins>"#;
    let body = format!("<w:p>{ins}</w:p>");
    assert_eq!(zip_has_marks(strict(&body, "")), Some(true));
    assert_eq!(zip_has_marks(strict("<w:p/>", ins)), Some(true));
    assert_eq!(zip_has_marks(strict(&body, ins)), Some(true));
    assert_eq!(zip_has_marks(strict("<w:p/>", "")), Some(false));
    let part = format!(
        r#"<x:document xmlns:x="{STRICT}"><x:body><x:del x:id="2"/></x:body></x:document>"#
    );
    assert_eq!(
        holds_any(part.as_bytes(), &[CHANGE_MARKS]),
        Some(Found::Mark)
    );
}

/// quick-xml binds a prefix to the value as written; Word decodes it. A reference in a namespace
/// declaration may have changes, whichever namespace it spells.
#[test]
fn a_reference_in_a_namespace_declaration_may_have_changes() {
    let spelt = "http://schemas.openxmlformats.org/wordprocessingml/2006/mai&#110;";
    let part =
        format!(r#"<w:document xmlns:w="{spelt}"><w:body><w:ins w:id="1"/></w:body></w:document>"#);
    assert_eq!(
        holds_any(part.as_bytes(), &[CHANGE_MARKS]),
        Some(Found::Unmodelled)
    );
    let clean = format!(r#"<w:document xmlns:w="{spelt}"><w:body/></w:document>"#);
    assert_eq!(
        holds_any(clean.as_bytes(), &[CHANGE_MARKS]),
        Some(Found::Unmodelled)
    );
    assert_eq!(
        zip_has_marks(docx(&[("word/document.xml", part.as_str())])),
        Some(true)
    );
}

/// A document whose relationships part is `rels`, with a clean header and `extra` parts.
fn named(rels: &str, header: &str, extra: &[(&str, &str)]) -> Option<bool> {
    let mut parts = vec![
        ("word/document.xml", "<w:body><w:p/></w:body>"),
        ("word/_rels/document.xml.rels", rels),
        ("word/header1.xml", header),
    ];
    parts.extend_from_slice(extra);
    zip_has_marks(docx(&parts))
}

/// The relationships read as XML, only to notice a missing part: a reference in a target is
/// decoded, a prefixed element read, a target matched without case; what they name or not
/// changes nothing about what is read.
#[test]
fn the_relationships_are_read_as_xml_and_decide_nothing() {
    let clean = "<w:hdr><w:p/></w:hdr>";
    let changed = r#"<w:hdr><w:p><w:ins w:id="1"/></w:p></w:hdr>"#;
    let rel = |attributes: &str| {
        format!(
            r#"<Relationships xmlns="{RELS}"><Relationship Id="rId1" {attributes}/></Relationships>"#
        )
    };
    // a type and a target spelt with references
    let referenced = rel(
        r#"Type="http://x/&#104;&#101;&#97;&#100;&#101;&#114;" Target="&#104;&#101;&#97;&#100;&#101;&#114;1.xml""#,
    );
    assert_eq!(named(&referenced, clean, &[]), Some(false));
    assert_eq!(named(&referenced, changed, &[]), Some(true));
    // a target differing only in case names the same part
    let cased = rel(r#"Type="http://x/header" Target="Header1.XML""#);
    assert_eq!(named(&cased, clean, &[]), Some(false));
    assert_eq!(named(&cased, changed, &[]), Some(true));
    // a prefixed element, naming a part the zip lacks
    let prefixed = format!(
        r#"<r:Relationships xmlns:r="{RELS}"><r:Relationship Id="rId1" Type="http://x/header" Target="header9.xml"/></r:Relationships>"#
    );
    assert_eq!(named(&prefixed, clean, &[]), Some(true));
    // named by nothing: read all the same
    assert_eq!(
        named(&rel(r#"Type="t" Target="header1.xml""#), changed, &[]),
        Some(true)
    );
    let empty = format!(r#"<Relationships xmlns="{RELS}"/>"#);
    assert_eq!(named(&empty, changed, &[]), Some(true));
}

/// A part outside `word/`, or XML under another name that the content types give an XML type,
/// is read like any other.
#[test]
fn a_part_outside_word_or_not_named_xml_is_read() {
    let empty = format!(r#"<Relationships xmlns="{RELS}"/>"#);
    let clean = "<w:hdr><w:p/></w:hdr>";
    let changed = r#"<w:hdr><w:p><w:del w:id="1"/></w:p></w:hdr>"#;
    assert_eq!(
        named(&empty, clean, &[("custom/head.xml", changed)]),
        Some(true)
    );
    let types = TYPES.replace(
        "</Types>",
        r#"<Override PartName="/custom/head.bin" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#,
    );
    // not named `.xml`, so bound here as Word binds it
    let (bin_changed, bin_clean) = (declared(changed), declared(clean));
    let typed = [
        ("[Content_Types].xml", types.as_str()),
        ("custom/head.bin", bin_changed.as_str()),
    ];
    assert_eq!(named(&empty, clean, &typed), Some(true));
    let typed_clean = [
        ("[Content_Types].xml", types.as_str()),
        ("custom/head.bin", bin_clean.as_str()),
    ];
    assert_eq!(named(&empty, clean, &typed_clean), Some(false));
    // a part the content types name and the zip lacks
    assert_eq!(named(&empty, clean, &typed[..1]), Some(true));
    // no content types at all
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    std::io::Write::write_all(&mut zip, b"<w:document/>").unwrap();
    let mut file = zip.finish().unwrap();
    file.set_position(0);
    assert_eq!(zip_has_marks(file), Some(true));
}

/// A package's relationships past 1 MiB naming its main document at `/x/main.xml`: the part is
/// read whatever the relationships' size, and their size is no reason to stop.
#[test]
fn a_main_document_outside_word_behind_large_relationships_is_read() {
    let mut package = format!(r#"<Relationships xmlns="{RELS}">"#);
    for n in 0..15_000 {
        write!(
            package,
            r#"<Relationship Id="rId{n}" Type="http://x/officeDocument" Target="/x/main.xml"/>"#
        )
        .unwrap();
    }
    package.push_str("</Relationships>");
    assert!(package.len() > 1024 * 1024);
    let file = |body: &str| {
        let main = format!("<w:document><w:body>{body}</w:body></w:document>");
        zip_has_marks(docx(&[
            ("_rels/.rels", package.as_str()),
            ("x/main.xml", main.as_str()),
        ]))
    };
    assert_eq!(file(r#"<w:p><w:ins w:id="1"/></w:p>"#), Some(true));
    assert_eq!(file("<w:p/>"), Some(false));
}

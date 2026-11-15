//! The marks scan reads each part as XML: the ways well-formed XML could hide a change from a
//! byte scan each find it, a part the scan does not model may have changes, and a clean UTF-8
//! part is still clean.

use std::io::{Cursor, Write as _};

use super::tests::docx;
use super::{CHANGE_MARKS, Found, holds_any, zip_has_marks};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// What the scan finds in `xml`, read as one part.
fn scan(xml: &[u8]) -> Option<Found> {
    holds_any(xml, &[CHANGE_MARKS])
}

#[test]
fn the_namespace_bound_on_a_nested_element_is_read() {
    let xml = format!(
        r#"<a:root xmlns:a="urn:other"><a:body><x:p xmlns:x="{W}"><x:ins x:id="1"/></x:p></a:body></a:root>"#
    );
    assert_eq!(scan(xml.as_bytes()), Some(Found::Mark));
    // `w:` bound to another namespace names no change
    let other = r#"<w:document xmlns:w="urn:other"><w:ins w:id="1"/></w:document>"#;
    assert_eq!(scan(other.as_bytes()), Some(Found::Clean));
    let default = format!(r#"<document xmlns="{W}"><body><del id="2"/></body></document>"#);
    assert_eq!(scan(default.as_bytes()), Some(Found::Mark));
}

#[test]
fn a_root_past_the_first_64_kib_is_read() {
    let comment = "x".repeat(70 * 1024);
    let xml = format!(
        r#"<?xml version="1.0"?><!-- {comment} --><x:document xmlns:x="{W}"><x:ins x:id="1"/></x:document>"#
    );
    assert_eq!(scan(xml.as_bytes()), Some(Found::Mark));
}

#[test]
fn a_greater_than_sign_in_a_root_attribute_is_read_past() {
    let xml = format!(r#"<x:document a="1>2" xmlns:x="{W}"><x:ins x:id="1"/></x:document>"#);
    assert_eq!(scan(xml.as_bytes()), Some(Found::Mark));
}

#[test]
fn a_comment_opening_inside_a_processing_instruction_hides_nothing() {
    let xml = format!(
        r#"<?xml version="1.0"?><?mso-application <!-- ?><w:document xmlns:w="{W}"><w:ins w:id="1"/><!-- --></w:document>"#
    );
    assert_eq!(scan(xml.as_bytes()), Some(Found::Mark));
}

#[test]
fn a_change_in_a_comment_or_cdata_is_text() {
    let xml = format!(
        r#"<w:document xmlns:w="{W}"><!-- <w:ins w:id="1"> --><w:t><![CDATA[<w:del w:id="2">]]></w:t></w:document>"#
    );
    assert_eq!(scan(xml.as_bytes()), Some(Found::Clean));
}

#[test]
fn a_part_not_in_utf_8_may_have_changes() {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?><w:document xmlns:w="{W}"><w:ins w:id="1"/></w:document>"#
    );
    let le: Vec<u8> = xml.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let be: Vec<u8> = xml.encode_utf16().flat_map(u16::to_be_bytes).collect();
    for (what, bytes) in [
        (
            "little-endian with its mark",
            [&[0xFF, 0xFE][..], &le[..]].concat(),
        ),
        (
            "big-endian with its mark",
            [&[0xFE, 0xFF][..], &be[..]].concat(),
        ),
        ("little-endian without", le),
        ("big-endian without", be),
    ] {
        assert_eq!(scan(&bytes), Some(Found::Unmodelled), "{what}");
    }
    // UTF-8 bytes that say they are another encoding
    let latin = xml
        .replace("UTF-16", "ISO-8859-1")
        .replace("<w:ins w:id=\"1\"/>", "");
    assert_eq!(scan(latin.as_bytes()), Some(Found::Unmodelled));
}

#[test]
fn what_the_scan_does_not_model_may_have_changes() {
    for xml in [
        // a prefix never declared
        "<w:document><w:body/></w:document>".to_owned(),
        // not well-formed
        format!(r#"<w:document xmlns:w="{W}"><w:body></w:document>"#),
        // a doctype, whose entities could hold markup
        format!(
            r#"<!DOCTYPE d [<!ENTITY e "<w:ins w:id='1'/>">]><w:document xmlns:w="{W}">&e;</w:document>"#
        ),
    ] {
        assert_eq!(scan(xml.as_bytes()), Some(Found::Unmodelled), "{xml}");
    }
}

#[test]
fn a_clean_utf_8_part_is_clean() {
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<?mso-application progid="Word.Document"?>
<w:document xmlns:wpc="urn:wpc" xmlns:w="{W}" xmlns:r="urn:r" mc:Ignorable="w14"
 xmlns:mc="urn:mc"><w:body><w:p><w:r><w:t xml:space="preserve">Clause 4 &amp; 5 —</w:t></w:r></w:p>
<w:sectPr><w:pgSz w:w="11906"/></w:sectPr></w:body></w:document>"#
    );
    assert_eq!(scan(body.as_bytes()), Some(Found::Clean));
    // with UTF-8's byte order mark
    let marked = [&[0xEF, 0xBB, 0xBF][..], body.as_bytes()].concat();
    assert_eq!(scan(&marked), Some(Found::Clean));
    assert_eq!(
        zip_has_marks(docx(&[("word/document.xml", body.as_str())])),
        Some(false)
    );
}

/// A part that may have changes offers Tracked changes, as a change does; the file's answer is
/// `Some(true)`.
#[test]
fn a_file_with_a_part_it_does_not_model_answers_true() {
    let utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(
            format!(r#"<w:document xmlns:w="{W}"><w:body/></w:document>"#)
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        )
        .collect();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(&utf16).unwrap();
    let mut file = zip.finish().unwrap();
    file.set_position(0);
    assert_eq!(zip_has_marks(file), Some(true));
    // a prefix never declared, here as in a part
    let undeclared = "<x:document><x:body/></x:document>";
    assert_eq!(
        zip_has_marks(docx(&[("word/document.xml", undeclared)])),
        Some(true)
    );
}

use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

use super::{CHANGE_MARKS, COMMENT_MARKS, MAX_XML_BYTES, check, holds_any, zip_has_marks};
use crate::pipeline::tests::support::TempDir;

/// A reader that hands out one byte at a time, so every mark spans two reads.
struct Trickle<'a>(&'a [u8]);

impl std::io::Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let Some((first, rest)) = self.0.split_first() else {
            return Ok(0);
        };
        if let Some(slot) = buf.first_mut() {
            *slot = *first;
            self.0 = rest;
            return Ok(1);
        }
        Ok(0)
    }
}

/// A reader whose read fails after `ok` bytes, as a locked or vanished file's does.
struct Fails<'a>(&'a [u8]);

impl std::io::Read for Fails<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.0.is_empty() {
            return Err(std::io::Error::other("a sharing violation"));
        }
        let n = self.0.len().min(buf.len());
        buf[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

#[test]
fn a_mark_is_found_across_reads_and_a_near_miss_is_not() {
    let xml = b"<w:p><w:ins w:id=\"1\" w:author=\"A\"><w:r/></w:ins></w:p>";
    assert_eq!(holds_any(Trickle(xml), &[CHANGE_MARKS]), Some(true));
    assert_eq!(holds_any(&xml[..], &[CHANGE_MARKS]), Some(true));
    let moved = b"<w:p><w:moveFrom w:id=\"3\"/></w:p>";
    assert_eq!(holds_any(Trickle(moved), &[CHANGE_MARKS]), Some(true));
    let plain = b"<w:p><w:r><w:t>w:ins and w:del are words here</w:t></w:r><w:instrText/></w:p>";
    assert_eq!(holds_any(&plain[..], &[CHANGE_MARKS]), Some(false));
    assert_eq!(holds_any(Trickle(plain), &[CHANGE_MARKS]), Some(false));
    let comment = b"<w:comments><w:comment w:id=\"0\">";
    assert_eq!(holds_any(&comment[..], &[COMMENT_MARKS]), Some(true));
    let none = b"<w:comments></w:comments>";
    assert_eq!(holds_any(&none[..], &[COMMENT_MARKS]), Some(false));
}

#[test]
fn a_read_that_fails_is_unknown_not_unmarked() {
    let clean = b"<w:body><w:p><w:r><w:t>Clause 4</w:t></w:r></w:p>";
    assert_eq!(holds_any(Fails(clean), &[CHANGE_MARKS]), None);
    // A mark before the failure still counts.
    let changed = b"<w:body><w:del w:id=\"2\"/>";
    assert_eq!(holds_any(Fails(changed), &[CHANGE_MARKS]), Some(true));
}

/// A main document's relationships part naming these `(type, target)` relationships.
fn rels(targets: &[(&str, &str)]) -> String {
    let mut xml = String::from("<Relationships>");
    for (n, (kind, target)) in targets.iter().enumerate() {
        let kind =
            format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}");
        write!(
            xml,
            r#"<Relationship Id="rId{n}" Type="{kind}" Target="{target}"/>"#
        )
        .unwrap();
    }
    xml.push_str("</Relationships>");
    xml
}

/// A `.docx` with these parts, in memory.
fn docx(parts: &[(&str, &str)]) -> Cursor<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in parts {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    let mut file = zip.finish().unwrap();
    file.set_position(0);
    file
}

const BODY: &str = "<w:body><w:p><w:r><w:t>Clause 4</w:t></w:r></w:p></w:body>";

#[test]
fn a_word_file_has_marks_by_its_changes_or_its_comments_only() {
    assert_eq!(
        zip_has_marks(docx(&[("word/document.xml", BODY)])),
        Some(false)
    );
    let changed = "<w:body><w:p><w:del w:id=\"2\" w:author=\"A\"/></w:p></w:body>";
    assert_eq!(
        zip_has_marks(docx(&[("word/document.xml", changed)])),
        Some(true)
    );
    let comments = "<w:comments><w:comment w:id=\"0\" w:author=\"A\"/></w:comments>";
    let rels = rels(&[("comments", "comments.xml")]);
    let parts = [
        ("word/document.xml", BODY),
        ("word/_rels/document.xml.rels", rels.as_str()),
        ("word/comments.xml", comments),
    ];
    assert_eq!(zip_has_marks(docx(&parts)), Some(true));
    // A comments part the document's relationships do not name is not one stetdoc reads.
    assert_eq!(zip_has_marks(docx(&[parts[0], parts[2]])), Some(false));
    assert_eq!(
        zip_has_marks(Cursor::new(b"not a zip".to_vec())),
        Some(false)
    );
}

#[test]
fn a_file_that_will_not_open_is_unknown() {
    let dir = TempDir::new("marks_unopened");
    assert_eq!(check(&dir.path().join("gone.docx")), None);
}

#[cfg(unix)]
#[test]
fn a_link_is_never_followed() {
    let dir = TempDir::new("marks_link");
    let changed = "<w:body><w:p><w:ins w:id=\"1\"/></w:p></w:body>";
    let real = dir.path().join("real.docx");
    crate::write::write_file(&real, docx(&[("word/document.xml", changed)]).get_ref()).unwrap();
    assert_eq!(check(&real), Some(true));
    let link = dir.path().join("link.docx");
    crate::write::symlink(&real, &link).unwrap();
    // O_NOFOLLOW refuses the link (ELOOP): unknown, never the target's answer.
    assert_eq!(check(&link), None);
}

/// The repro files from the field, as made (`tests/fixtures/revisions/README.md`): every tracked
/// change of the first is in a header, a footer, a note or formatting, none an insertion,
/// deletion or move of the body's text; the second adds one body insertion.
const NONBODY_ONLY: &[u8] =
    include_bytes!("../../../tests/fixtures/revisions/tracked-nonbody-only.docx");
const NONBODY_AND_FORMAT: &[u8] =
    include_bytes!("../../../tests/fixtures/revisions/tracked-nonbody-and-format.docx");

#[test]
fn the_field_s_files_with_changes_outside_the_body_have_marks() {
    assert_eq!(
        zip_has_marks(Cursor::new(NONBODY_ONLY.to_vec())),
        Some(true)
    );
    assert_eq!(
        zip_has_marks(Cursor::new(NONBODY_AND_FORMAT.to_vec())),
        Some(true)
    );
}

const AUTHOR: &str = r#"w:author="R" w:date="2026-11-01T00:00:00Z""#;

/// One tracked change of each kind stetdoc 0.9.3 lists, accepts and rejects, as Word writes it,
/// by the element that marks it.
fn every_kind() -> Vec<(&'static str, String)> {
    let a = AUTHOR;
    let cell = |tc_pr: &str| {
        format!(
            "<w:tbl><w:tr><w:tc><w:tcPr><w:tcW w:w=\"4000\"/>{tc_pr}</w:tcPr><w:p/></w:tc></w:tr></w:tbl>"
        )
    };
    vec![
        (
            "ins",
            format!("<w:p><w:ins w:id=\"1\" {a}><w:r><w:t>new</w:t></w:r></w:ins></w:p>"),
        ),
        (
            "del",
            format!(
                "<w:p><w:del w:id=\"1\" {a}><w:r><w:delText>old</w:delText></w:r></w:del></w:p>"
            ),
        ),
        (
            "moveFrom",
            format!("<w:p><w:moveFrom w:id=\"1\" {a}><w:r/></w:moveFrom></w:p>"),
        ),
        (
            "moveTo",
            format!("<w:p><w:moveTo\n w:id=\"1\" {a}><w:r/></w:moveTo></w:p>"),
        ),
        (
            "paragraph mark",
            format!("<w:p><w:pPr><w:rPr><w:ins w:id=\"1\" {a}/></w:rPr></w:pPr></w:p>"),
        ),
        (
            "rPrChange",
            format!(
                "<w:r><w:rPr><w:b/><w:rPrChange w:id=\"1\" {a}><w:rPr/></w:rPrChange></w:rPr></w:r>"
            ),
        ),
        (
            "pPrChange",
            format!(
                "<w:p><w:pPr><w:jc w:val=\"center\"/><w:pPrChange w:id=\"1\" {a}><w:pPr/></w:pPrChange></w:pPr></w:p>"
            ),
        ),
        (
            "sectPrChange",
            format!(
                "<w:sectPr><w:sectPrChange w:id=\"1\" {a}><w:sectPr/></w:sectPrChange></w:sectPr>"
            ),
        ),
        (
            "tblPrChange",
            format!(
                "<w:tbl><w:tblPr><w:tblPrChange w:id=\"1\" {a}><w:tblPr/></w:tblPrChange></w:tblPr></w:tbl>"
            ),
        ),
        (
            "tblPrExChange",
            format!(
                "<w:tr><w:tblPrEx><w:tblPrExChange w:id=\"1\" {a}><w:tblPrEx/></w:tblPrExChange></w:tblPrEx></w:tr>"
            ),
        ),
        (
            "tblGridChange",
            "<w:tblGrid><w:tblGridChange w:id=\"1\"><w:tblGrid/></w:tblGridChange></w:tblGrid>"
                .to_owned(),
        ),
        (
            "trPrChange",
            format!(
                "<w:tr><w:trPr><w:trPrChange w:id=\"1\" {a}><w:trPr/></w:trPrChange></w:trPr></w:tr>"
            ),
        ),
        (
            "tcPrChange",
            cell(&format!(
                "<w:tcPrChange w:id=\"1\" {a}><w:tcPr/></w:tcPrChange>"
            )),
        ),
        (
            "numberingChange",
            format!("<w:numPr><w:numberingChange w:id=\"1\" {a} w:original=\"%1.\"/></w:numPr>"),
        ),
        (
            "row inserted",
            format!("<w:tr><w:trPr><w:ins w:id=\"1\" {a}/></w:trPr></w:tr>"),
        ),
        ("cellIns", cell(&format!("<w:cellIns w:id=\"1\" {a}/>"))),
        ("cellDel", cell(&format!("<w:cellDel\tw:id=\"1\" {a}/>"))),
        (
            "cellMerge",
            cell(&format!("<w:cellMerge w:id=\"1\" {a} w:vMerge=\"rest\"/>")),
        ),
        (
            "customXmlInsRangeStart",
            format!("<w:customXmlInsRangeStart w:id=\"1\" {a}/>"),
        ),
        (
            "customXmlDelRangeStart",
            format!("<w:customXmlDelRangeStart w:id=\"1\" {a}/>"),
        ),
    ]
}

/// What a part of each story holds when it holds no change: its root, and content stetdoc reads
/// as no change (a comment holds no `<w:comment `, so the comments part's own mark is not met).
const PARTS: &[(&str, &str, &str)] = &[
    (
        "document",
        "word/document.xml",
        "<w:document><w:body>{}<w:p/></w:body></w:document>",
    ),
    ("header", "word/header1.xml", "<w:hdr>{}<w:p/></w:hdr>"),
    ("footer", "word/footer2.xml", "<w:ftr>{}<w:p/></w:ftr>"),
    (
        "footnotes",
        "word/footnotes.xml",
        "<w:footnotes><w:footnote w:id=\"1\">{}</w:footnote></w:footnotes>",
    ),
    (
        "endnotes",
        "word/endnotes.xml",
        "<w:endnotes><w:endnote w:id=\"1\">{}</w:endnote></w:endnotes>",
    ),
    (
        "comments",
        "word/comments.xml",
        "<w:comments>{}</w:comments>",
    ),
];

/// A whole Word file, every story part named by the relationships Word writes, with `change`
/// in the part of the story `story` and none anywhere else.
fn word_file(story: &str, change: &str) -> Cursor<Vec<u8>> {
    let package = r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let document = rels(&[
        ("styles", "styles.xml"),
        ("header", "header1.xml"),
        ("footer", "/word/footer2.xml"),
        ("footnotes", "footnotes.xml"),
        ("endnotes", "endnotes.xml"),
        ("comments", "comments.xml"),
    ]);
    let styles = "<w:styles><w:style><w:rPr><w:rPrChange w:id=\"9\"/></w:rPr></w:style></w:styles>";
    let mut parts: Vec<(&str, String)> = vec![
        ("_rels/.rels", package.to_owned()),
        ("word/_rels/document.xml.rels", document),
        // stetdoc reads no change in the styles: neither does the scan
        ("word/styles.xml", styles.to_owned()),
    ];
    for (name, part, root) in PARTS {
        let inside = if *name == story { change } else { "" };
        parts.push((*part, root.replace("{}", inside)));
    }
    let parts: Vec<(&str, &str)> = parts
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    docx(&parts)
}

#[test]
fn every_kind_of_change_in_every_story_part_is_a_mark() {
    assert_eq!(zip_has_marks(word_file("none", "")), Some(false));
    for (story, _, _) in PARTS {
        for (kind, change) in every_kind() {
            assert_eq!(
                zip_has_marks(word_file(story, &change)),
                Some(true),
                "{kind} in the {story}"
            );
        }
    }
}

#[test]
fn a_near_miss_or_a_part_stetdoc_does_not_read_is_no_mark() {
    for near in [
        "<w:tblBorders><w:insideH w:val=\"single\"/></w:tblBorders>",
        "<w:r><w:delInstrText xml:space=\"preserve\">PAGE</w:delInstrText></w:r>",
        "<w:r><w:instrText>PAGE</w:instrText><w:commentReference w:id=\"0\"/></w:r>",
        // a move's range alone is no change stetdoc lists
        "<w:moveFromRangeStart w:id=\"1\" w:name=\"m\"/><w:moveFromRangeEnd w:id=\"1\"/>",
    ] {
        assert_eq!(
            zip_has_marks(word_file("header", near)),
            Some(false),
            "{near}"
        );
    }
    // A header the document's relationships do not name, or name as outside the package.
    let change = format!("<w:hdr><w:p><w:ins w:id=\"1\" {AUTHOR}/></w:p></w:hdr>");
    let external = r#"<Relationships><Relationship Id="rId1" Type="http://x/header" Target="header1.xml" TargetMode="External"/></Relationships>"#;
    for rels in ["<Relationships/>", external] {
        let parts = [
            ("word/document.xml", BODY),
            ("word/_rels/document.xml.rels", rels),
            ("word/header1.xml", change.as_str()),
        ];
        assert_eq!(zip_has_marks(docx(&parts)), Some(false), "{rels}");
    }
}

#[test]
fn the_main_document_is_where_the_package_says() {
    let package = r#"<Relationships><Relationship Id="r" Type="http://x/relationships/officeDocument" Target="/word/document2.xml"/></Relationships>"#;
    let changed = "<w:body><w:p><w:del w:id=\"2\" w:author=\"A\"/></w:p></w:body>";
    let parts = [
        ("_rels/.rels", package),
        ("word/document.xml", BODY),
        ("word/document2.xml", changed),
    ];
    assert_eq!(zip_has_marks(docx(&parts)), Some(true));
}

/// Clean parts past the scan's total stop it: a change in the sixth header, after five of 16 MiB,
/// is never read.
#[test]
fn the_scan_stops_after_its_total_read() {
    let filler = "<w:p/>".repeat(usize::try_from(MAX_XML_BYTES).unwrap() / 6);
    let change = format!("<w:hdr><w:p><w:ins w:id=\"1\" {AUTHOR}/></w:p></w:hdr>");
    let targets: Vec<(String, String)> = (1..=6)
        .map(|n| ("header".to_owned(), format!("header{n}.xml")))
        .collect();
    let targets: Vec<(&str, &str)> = targets
        .iter()
        .map(|(k, t)| (k.as_str(), t.as_str()))
        .collect();
    let document_rels = rels(&targets);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    let mut put = |name: &str, text: &str| {
        zip.start_file(name, stored).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    };
    put("word/document.xml", BODY);
    put("word/_rels/document.xml.rels", &document_rels);
    for n in 1..=5 {
        put(&format!("word/header{n}.xml"), &filler);
    }
    put("word/header6.xml", &change);
    let mut file = zip.finish().unwrap();
    file.set_position(0);
    assert_eq!(zip_has_marks(file), Some(false));
}

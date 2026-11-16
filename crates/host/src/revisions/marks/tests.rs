use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

use std::time::Duration;

use super::parts::MAX_PARTS;
use super::{
    CHANGE_MARKS, COMMENT_MARKS, Found, W_NAMESPACES, check, holds_any, zip_has_marks, zip_marks,
};
use crate::clock::{Clock as _, SystemClock};
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

/// `xml` with Word's main namespace bound to `w` on its first element, as Word writes every part;
/// as it is when it binds `w` itself.
pub(super) fn declared(xml: &str) -> String {
    let Some((before, rest)) = xml.split_once("<w:").filter(|_| !xml.contains("xmlns:w=")) else {
        return xml.to_owned();
    };
    let end = rest
        .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at_checked(end).unwrap();
    let namespace = W_NAMESPACES[0];
    format!("{before}<w:{name} xmlns:w=\"{namespace}\"{after}")
}

#[test]
fn a_mark_is_found_across_reads_and_a_near_miss_is_not() {
    let found = |xml: &str, marks| {
        let xml = declared(xml);
        let whole = holds_any(xml.as_bytes(), &[marks]);
        assert_eq!(holds_any(Trickle(xml.as_bytes()), &[marks]), whole, "{xml}");
        whole
    };
    let xml = "<w:p><w:ins w:id=\"1\" w:author=\"A\"><w:r/></w:ins></w:p>";
    assert_eq!(found(xml, CHANGE_MARKS), Some(Found::Mark));
    let moved = "<w:p><w:moveFrom w:id=\"3\"/></w:p>";
    assert_eq!(found(moved, CHANGE_MARKS), Some(Found::Mark));
    let plain = "<w:p><w:r><w:t>w:ins and w:del are words here</w:t></w:r><w:instrText/></w:p>";
    assert_eq!(found(plain, CHANGE_MARKS), Some(Found::Clean));
    let comment = "<w:comments><w:comment w:id=\"0\">";
    assert_eq!(found(comment, COMMENT_MARKS), Some(Found::Mark));
    let none = "<w:comments></w:comments>";
    assert_eq!(found(none, COMMENT_MARKS), Some(Found::Clean));
}

#[test]
fn a_read_that_fails_is_unknown_not_unmarked() {
    let clean = declared("<w:body><w:p><w:r><w:t>Clause 4</w:t></w:r></w:p>");
    assert_eq!(holds_any(Fails(clean.as_bytes()), &[CHANGE_MARKS]), None);
    // A mark before the failure still counts.
    let changed = declared("<w:body><w:del w:id=\"2\"/>");
    assert_eq!(
        holds_any(Fails(changed.as_bytes()), &[CHANGE_MARKS]),
        Some(Found::Mark)
    );
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

/// The content types a test file has unless it gives its own: `.xml` is XML, `.rels` relationships.
pub(super) const TYPES: &str = concat!(
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" "#,
    r#"ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="xml" ContentType="application/xml"/></Types>"#,
);

/// A `.docx` with these parts, in memory, each XML part [`declared`], and [`TYPES`] first unless
/// it gives `[Content_Types].xml`.
pub(super) fn docx(parts: &[(&str, &str)]) -> Cursor<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let typed = parts.iter().any(|(name, _)| *name == "[Content_Types].xml");
    let types = [("[Content_Types].xml", TYPES)];
    let types = if typed { &[][..] } else { &types[..] };
    for (name, text) in types.iter().chain(parts) {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        let xml = std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"));
        let text = if xml {
            declared(text)
        } else {
            (*text).to_owned()
        };
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
    // a part the relationships name and the zip lacks: the file may have changes
    assert_eq!(zip_has_marks(docx(&[parts[0], parts[1]])), Some(true));
    assert_eq!(zip_has_marks(docx(&parts)), Some(true));
    // the relationships decide nothing: a comments part nothing names is read all the same
    assert_eq!(zip_has_marks(docx(&[parts[0], parts[2]])), Some(true));
    // a file that does not open as a zip may have changes
    assert_eq!(
        zip_has_marks(Cursor::new(b"not a zip".to_vec())),
        Some(true)
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
    let styles = "<w:styles><w:style><w:rPr><w:b/></w:rPr></w:style></w:styles>";
    let mut parts: Vec<(&str, String)> = vec![
        ("_rels/.rels", package.to_owned()),
        ("word/_rels/document.xml.rels", document),
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
fn a_near_miss_is_no_mark_and_every_part_is_read_whatever_names_it() {
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
    // A header the document's relationships do not name, or name as outside the package, or a
    // change in the styles, which stetdoc does not list: read all the same, offered.
    let change = format!("<w:hdr><w:p><w:ins w:id=\"1\" {AUTHOR}/></w:p></w:hdr>");
    let external = r#"<Relationships><Relationship Id="rId1" Type="http://x/header" Target="header1.xml" TargetMode="External"/></Relationships>"#;
    for rels in ["<Relationships/>", external] {
        let parts = [
            ("word/document.xml", BODY),
            ("word/_rels/document.xml.rels", rels),
            ("word/header1.xml", change.as_str()),
        ];
        assert_eq!(zip_has_marks(docx(&parts)), Some(true), "{rels}");
    }
    let styles = "<w:styles><w:style><w:rPr><w:rPrChange w:id=\"9\"/></w:rPr></w:style></w:styles>";
    let parts = [("word/document.xml", BODY), ("word/styles.xml", styles)];
    assert_eq!(zip_has_marks(docx(&parts)), Some(true));
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

/// A header of exactly `size` bytes, holding no change.
fn clean_header(size: usize) -> String {
    let open = declared("<w:hdr>");
    let close = "</w:hdr>";
    let room = size - open.len() - close.len();
    let paragraphs = "<w:p/>".repeat(room / 6);
    format!("{open}{paragraphs}{}{close}", " ".repeat(room % 6))
}

/// A file whose document names `headers`, as `(target, text)`, the headers its last parts.
fn with_headers(headers: &[(String, String)]) -> Cursor<Vec<u8>> {
    let targets: Vec<(&str, &str)> = headers
        .iter()
        .map(|(t, _)| ("header", t.as_str()))
        .collect();
    let document_rels = rels(&targets);
    let mut parts = vec![
        ("word/document.xml".to_owned(), BODY.to_owned()),
        ("word/_rels/document.xml.rels".to_owned(), document_rels),
    ];
    parts.extend(
        headers
            .iter()
            .map(|(t, text)| (format!("word/{t}"), text.clone())),
    );
    let parts: Vec<(&str, &str)> = parts
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    docx(&parts)
}

/// The scan never answers "no changes" for what it did not read whole: a part past its cap, parts
/// left unread past the total cap, or more XML parts than it reads. Each may have changes,
/// which offers Tracked changes; stetdoc decides when they are listed. Read whole, a clean file is
/// still clean. (The caps here are small stand-ins for the real ones, which are bytes alike.)
#[test]
fn what_the_scan_could_not_read_whole_may_have_changes() {
    let header = |n: usize, size: usize| (format!("header{n}.xml"), clean_header(size));
    let one = with_headers(&[header(1, 4_000)]);
    // a part over the part cap; exactly at it, read whole
    assert_eq!(zip_marks(one.clone(), 3_999, u64::MAX), Some(true));
    assert_eq!(zip_marks(one, 4_000, u64::MAX), Some(false));
    // the total cap: what is read before the headers, then two headers of 4,000 bytes
    let two = with_headers(&[header(1, 4_000), header(2, 4_000)]);
    let mut probe = zip::ZipArchive::new(two.clone()).unwrap();
    let others: Vec<String> = probe
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .filter(|name| !name.starts_with("word/header"))
        .collect();
    let before: u64 = others
        .iter()
        .map(|name| probe.by_name(name).unwrap().size())
        .sum();
    assert_eq!(zip_marks(two.clone(), 4_000, before + 8_000), Some(false));
    // the second header cut short by the total
    assert_eq!(zip_marks(two.clone(), 4_000, before + 7_999), Some(true));
    // the total used up at the first header's end, the second never opened
    assert_eq!(zip_marks(two, 4_000, before + 4_000), Some(true));
    // more XML parts than the scan reads: the document and n - 1 headers, named by nothing
    let parts = |n: usize| {
        let names: Vec<String> = (1..n).map(|k| format!("word/h{k}.xml")).collect();
        let header = declared("<w:hdr/>");
        let mut parts = vec![("word/document.xml", BODY)];
        parts.extend(names.iter().map(|name| (name.as_str(), header.as_str())));
        docx(&parts)
    };
    assert_eq!(zip_marks(parts(MAX_PARTS), 4_000, u64::MAX), Some(false));
    assert_eq!(zip_marks(parts(MAX_PARTS + 1), 4_000, u64::MAX), Some(true));
}

/// `<Relationship ` repeated inside a value, `size` bytes of it, in a relationships part that
/// names `target` as a header.
fn hostile_rels(size: usize, target: &str) -> String {
    let inside = "<Relationship ".repeat(size / "<Relationship ".len());
    format!(
        "<Relationships><Relationship Id=\"{inside}\" Target=\"{target}\" \
         Type=\"http://x/relationships/header\"/></Relationships>"
    )
}

/// Relationships parts shaped to make a byte scan read ahead from every match cost one pass, and
/// the header is read whatever they say.
#[test]
fn a_hostile_or_oversized_relationships_part_is_read_fast_and_capped() {
    let change = format!("<w:hdr><w:p><w:ins w:id=\"1\" {AUTHOR}/></w:p></w:hdr>");
    let cap = 1024 * 1024;
    let changed = "<w:body><w:p><w:del w:id=\"2\" w:author=\"A\"/></w:p></w:body>";
    for size in [256 * 1024, cap - 1024, 2 * cap] {
        let document_rels = hostile_rels(size, "header1.xml");
        let package = hostile_rels(size, "word/elsewhere.xml");
        let with_header = docx(&[
            ("_rels/.rels", package.as_str()),
            ("word/document.xml", BODY),
            ("word/_rels/document.xml.rels", document_rels.as_str()),
            ("word/header1.xml", change.as_str()),
        ]);
        let in_main = docx(&[
            ("_rels/.rels", package.as_str()),
            ("word/document.xml", changed),
        ]);
        let start = SystemClock.now().mono;
        let with_header = zip_has_marks(with_header);
        let in_main = zip_has_marks(in_main);
        let took = SystemClock.now().mono.saturating_sub(start);
        assert_eq!(with_header, Some(true), "{size}");
        assert_eq!(in_main, Some(true), "{size}");
        assert!(took < Duration::from_secs(1), "{size} bytes took {took:?}");
    }
}

//! zip_marks' generator: a small `.docx` built from the fuzz input, one byte per choice (a byte
//! past the input's end reads as 0, so a short input is a plain document). Each choice aims at a
//! way the scan could miss a mark: where the mark is, which namespace and how it is bound, what
//! the relationships and content types say, which parts are missing, and how the zip is broken.
//!
//! Input, after zip_marks' mode byte:
//!   0 flags: 1 Strict namespace, 2 prefixed (else default namespace), 4 bound on the root (else
//!     on the mark), 8 a character reference in the namespace value, 16 no content types,
//!     32 deflate, 64 lying relationships and content types, 128 an oversized part
//!   1 mark: one of [`MARKS`], then [`DECOYS`] (not marks), then none
//!   2 place: one of [`PLACES`]
//!   3 wrap: how the mark sits in its part ([`wrap`])
//!   4 main: where the main document is ([`MAINS`])
//!   5 oversize (with flag 128, when bits 16 and 32 are set): which part (top two bits), and by
//!     how much (low four)
//!   6 extra: 1 splice the tail raw (else escaped), 2 more clean parts, 4 the mark part first,
//!     8 Strict relationship types, 16 an alias prefix besides `w`
//!   7 corrupt, 8-9 where, 10 which character the reference replaces
//!   11 an SVG image: none, or one of [`SVGS`] (each built twice, the twin with a plain SVG)
//!   12 package: 1 an aFChunk relationship, 2 a second entry of the mark's part's name (4: in
//!     another case), 8 the SVG typed by part name (else by extension), 16 its type with a
//!     charset, 32 its type in capitals
//!   the rest: text spliced into the mark's part after the mark

use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

pub const T_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const S_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
/// A namespace that is not Word's, one character off.
const OTHER_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/mainx";

/// The marks marks.rs looks for (`CHANGE_MARKS` and `COMMENT_MARKS`).
pub const MARKS: &[&str] = &[
    "ins",
    "del",
    "moveFrom",
    "moveTo",
    "rPrChange",
    "pPrChange",
    "sectPrChange",
    "tblPrChange",
    "tblPrExChange",
    "tblGridChange",
    "trPrChange",
    "tcPrChange",
    "numberingChange",
    "cellIns",
    "cellDel",
    "cellMerge",
    "customXmlInsRangeStart",
    "customXmlDelRangeStart",
    "comment",
];

/// Names near a mark's that are not marks.
const DECOYS: &[&str] = &[
    "moveFromRangeStart",
    "moveToRangeEnd",
    "delText",
    "Ins",
    "insert",
    "commentReference",
    "commentRangeStart",
];

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const STRICT_REL: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/";

/// Where the mark's part is: its zip name relative to the main document's folder (or from the
/// zip's root, when it starts with `!`), and the relationship naming it, if any: type, target.
const PLACES: &[(&str, Option<(&str, &str)>)] = &[
    ("", None), // the main document
    ("header1.xml", Some(("header", "header1.xml"))),
    ("footnotes.xml", Some(("footnotes", "footnotes.xml"))),
    ("comments.xml", Some(("comments", "comments.xml"))),
    ("endnotes.xml", Some(("endnotes", "endnotes.xml"))),
    ("!word/footer2.xml", Some(("footer", "/word/footer2.xml"))),
    ("letterhead.xml", Some(("header", "letterhead.xml"))),
    ("Header1.XML", Some(("header", "Header1.XML"))),
    ("!word/letterhead.xml", None),
    ("!word/Header1.XML", None),
    ("!x/main.xml", None),
    ("!customXml/item1.xml", None),
    ("!word/glossary/document.xml", None),
    ("header3.xml", Some(("image", "header3.xml"))),
    ("![Content_Types].xml", None),
    ("header4.xml", Some(("header", "media/../header4.xml"))),
    ("!docProps/core.xml", None),
    ("!/word/header5.xml", Some(("header", "/word/header5.xml"))),
    ("!word\\header6.xml", Some(("header", "header6.xml"))),
    ("HEADER7.xml", Some(("header", "header7.xml"))),
    ("!main.xml", None),
    // a header saved under a relationships part's name, or another, typed as a header
    ("header8.rels", Some(("header", "header8.rels"))),
    ("story.bin", Some(("header", "story.bin"))),
];

/// A header's content type, for a story part named otherwise.
const HEADER_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";

/// An SVG Word never reads as a document part, but holding what the scan does not model.
const SVGS: &[&str] = &[
    "<?xml version=\"1.0\"?>\n<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \
     \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n\
     <svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    "<svg xmlns=\"http://www.w3.org/2000/svg\"><g></svg>",
    "<x:svg/>",
    "<svg xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:ins/></svg>",
    "<svg xmlns=\"http://www.w3.org/2000/sv&#103;\"/>",
];

/// The plain SVG of an odd one's twin.
const PLAIN_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>";

/// Where the main document is: the `officeDocument` target in `_rels/.rels` (none: no such
/// part), the zip name it is written at, and whether the type is Strict's.
const MAINS: &[(Option<&str>, &str, bool)] = &[
    (Some("word/document.xml"), "word/document.xml", false),
    (None, "word/document.xml", false),
    (Some("/x/main.xml"), "x/main.xml", false),
    (Some("word/main.xml"), "word/main.xml", false),
    (Some("word/document2.xml"), "word/document.xml", false),
    (Some("word/document.xml"), "word/document.xml", true),
];

/// One generated file: the zip, what was chosen, and what the generator knows about it.
pub struct Docx {
    pub zip: Vec<u8>,
    pub spec: String,
    /// Why #1068's design answers `Some(true)` for this file whatever its parts hold: a part the
    /// relationships or content types name is missing, an aFChunk, two entries of one name, no
    /// content types. Only set for a zip left whole.
    pub must_offer: Option<&'static str>,
    /// The same file with its odd SVG replaced by a plain one, which must answer the same.
    pub svg_twin: Option<Vec<u8>>,
}

/// The fuzz input as choices.
struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn u8(&mut self) -> u8 {
        let byte = self.data.get(self.at).copied().unwrap_or(0);
        self.at += 1;
        byte
    }

    fn pick(&mut self, n: usize) -> usize {
        usize::from(self.u8()) % n
    }

    fn rest(&self) -> &[u8] {
        self.data.get(self.at..).unwrap_or(&[])
    }
}

/// `uri`, with the character at `at` (modulo its length) as a character reference.
fn char_ref(uri: &str, at: u8) -> String {
    let at = usize::from(at) % uri.len();
    let c = uri.as_bytes()[at];
    let reference = if at % 2 == 0 {
        format!("&#{c};")
    } else {
        format!("&#x{c:X};")
    };
    format!("{}{reference}{}", &uri[..at], &uri[at + 1..])
}

/// The XML of `name` in the namespace value `ns`, with prefix `prefix` ("" for the default) and
/// the declaration on the element itself when `declare`.
fn element(prefix: &str, name: &str, ns: &str, declare: bool) -> String {
    let tag = if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}:{name}")
    };
    let decl = match (declare, prefix.is_empty()) {
        (false, _) => String::new(),
        (true, true) => format!(" xmlns=\"{ns}\""),
        (true, false) => format!(" xmlns:{prefix}=\"{ns}\""),
    };
    format!("<{tag}{decl} w:id=\"1\" w:author=\"A\"><w:r><w:t>x</w:t></w:r></{tag}>")
}

/// The mark's part: `mark` (an element's XML, or "") placed by the wrap choice. `root_decl` is
/// what the root element declares besides `w`.
fn wrap(choice: usize, mark: &str, root_decl: &str, name: &str, prefix: &str) -> String {
    let body = |inner: &str| format!("<w:body><w:p>{inner}</w:p></w:body>");
    let root = |decl: &str, inner: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
             <w:document xmlns:w=\"{T_NS}\"{decl}>{}</w:document>",
            body(inner)
        )
    };
    let p = if prefix.is_empty() { "q" } else { prefix };
    match choice % 13 {
        1 => {
            let open = "<w:sdt><w:sdtContent>".repeat(40);
            let close = "</w:sdtContent></w:sdt>".repeat(40);
            root(root_decl, &format!("{open}{mark}{close}"))
        }
        2 => root(root_decl, &format!("<!-- {mark} -->")),
        3 => root(
            root_decl,
            &format!("<w:r><w:t><![CDATA[{mark}]]></w:t></w:r>"),
        ),
        4 => root(root_decl, &format!("<?clauseo {mark}?>")),
        5 => root(
            root_decl,
            &format!("<{p}:r xmlns:{p}=\"{T_NS}\" {p}:{name}=\"1\"/>"),
        ),
        // the mark's prefix bound to another namespace on the mark itself
        6 => root(
            root_decl,
            &format!("<{p}:{name} xmlns:{p}=\"{OTHER_NS}\"/>"),
        ),
        // the mark's prefix bound elsewhere on the root, then to Word's on an ancestor
        7 => root(
            &format!(" xmlns:{p}=\"{OTHER_NS}\""),
            &format!("<w:sdt xmlns:{p}=\"{T_NS}\"><{p}:{name}/></w:sdt>"),
        ),
        8 => {
            let escaped = mark.replace('"', "&#34;");
            format!(
                "<?xml version=\"1.0\"?>\n<!DOCTYPE w:document [<!ENTITY m \"{escaped}\">]>\n\
                 <w:document xmlns:w=\"{T_NS}\"{root_decl}>{}</w:document>",
                body("&m;")
            )
        }
        // the default namespace undeclared around the mark
        9 => root(
            &format!(" xmlns=\"{T_NS}\""),
            &format!("<w:sdt xmlns=\"\"><{name}/></w:sdt>"),
        ),
        10 => format!("\u{feff}{}", root(root_decl, mark)),
        11 => root(root_decl, mark).replace("UTF-8", "UTF-16"),
        12 => format!("{}<!-- trailing -->\n", root(root_decl, mark)),
        _ => root(root_decl, mark),
    }
}

/// Text that makes a part over 1 MiB, the relationships cap, by `extra` more bytes.
fn padding(extra: usize) -> String {
    let mut text = String::from("<!--");
    text.push_str(&"padding ".repeat((1024 * 1024 + extra) / 8 + 1));
    text.push_str("-->");
    text
}

fn rels(entries: &[(String, String)], pad: &str) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships \
         xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for (n, (kind, target)) in entries.iter().enumerate() {
        let _ = write!(
            xml,
            "<Relationship Id=\"rId{n}\" Type=\"{kind}\" Target=\"{target}\"/>"
        );
    }
    xml.push_str(pad);
    xml.push_str("</Relationships>");
    xml
}

fn rels_of(name: &str) -> String {
    match name.rfind('/') {
        Some(at) => format!("{}_rels/{}.rels", &name[..=at], &name[at + 1..]),
        None => format!("_rels/{name}.rels"),
    }
}

/// The parts as zip name and text, written in order, with deflate when `deflate`; `dup` adds a
/// second entry of the part at `dup.0`, clean, of the same name (another case when `dup.1`).
fn write_zip(
    parts: &[(String, String)],
    deflate: bool,
    dup: Option<(usize, bool)>,
) -> Option<Vec<u8>> {
    let method = if deflate {
        CompressionMethod::Deflated
    } else {
        CompressionMethod::Stored
    };
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .large_file(false);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // the zip writer refuses a name twice: the copy is written under a stand-in of the same
    // length, replaced in the bytes after
    let mut stand_in = None;
    for (index, (name, text)) in parts.iter().enumerate() {
        zip.start_file(name.as_str(), options).ok()?;
        zip.write_all(text.as_bytes()).ok()?;
        if let Some((_, case)) = dup.filter(|&(at, _)| at == index) {
            let copy = if case && name.to_uppercase() != *name {
                name.to_uppercase()
            } else {
                let mut copy = name.clone();
                let last = if copy.pop() == Some('~') { '!' } else { '~' };
                copy.push(last);
                stand_in = Some((copy.clone(), name.clone()));
                copy
            };
            zip.start_file(copy.as_str(), options).ok()?;
            zip.write_all(wrap(0, "", "", "p", "").as_bytes()).ok()?;
        }
    }
    let mut bytes = zip.finish().ok()?.into_inner();
    if let Some((copy, name)) = stand_in {
        let (copy, name) = (copy.as_bytes(), name.as_bytes());
        let mut at = 0;
        while let Some(found) = bytes[at..].windows(copy.len()).position(|w| w == copy) {
            bytes[at + found..at + found + copy.len()].copy_from_slice(name);
            at += found + copy.len();
        }
    }
    Some(bytes)
}

/// The generated file for `data`, or `None` when the zip writer refused it.
pub fn build(data: &[u8]) -> Option<Docx> {
    let mut b = Bytes { data, at: 0 };
    let flags = b.u8();
    let mark_choice = b.pick(MARKS.len() + DECOYS.len() + 1);
    let place = b.pick(PLACES.len());
    let wrap_choice = b.pick(13);
    let main_choice = b.pick(MAINS.len());
    let over = b.u8();
    let extra = b.u8();
    let corrupt = b.u8();
    let pos = u16::from_le_bytes([b.u8(), b.u8()]);
    let ref_at = b.u8();
    let svg = b.pick(SVGS.len() + 1);
    let package = b.u8();
    let tail = String::from_utf8_lossy(b.rest()).into_owned();

    let strict = flags & 1 != 0;
    let ns = if strict { S_NS } else { T_NS };
    let ns_value = if flags & 8 != 0 {
        char_ref(ns, ref_at)
    } else {
        ns.to_owned()
    };
    let prefix = match (flags & 2 != 0, extra & 16 != 0) {
        (false, _) => "",
        (true, false) => "w2",
        (true, true) => "ns0",
    };
    let on_root = flags & 4 != 0;
    let name = MARKS
        .iter()
        .chain(DECOYS)
        .nth(mark_choice)
        .copied()
        .unwrap_or("p");
    let mark = if mark_choice < MARKS.len() + DECOYS.len() {
        element(prefix, name, &ns_value, !on_root)
    } else {
        String::new()
    };
    let root_decl = match (on_root, prefix.is_empty()) {
        (false, _) => String::new(),
        (true, true) => format!(" xmlns=\"{ns_value}\""),
        (true, false) => format!(" xmlns:{prefix}=\"{ns_value}\""),
    };
    let mut text = wrap(wrap_choice, &mark, &root_decl, name, prefix);
    let splice = if extra & 1 != 0 {
        tail.clone()
    } else {
        tail.replace('&', "&amp;").replace('<', "&lt;")
    };
    if let Some(at) = text.find("</w:p>") {
        text.insert_str(at, &splice);
    }

    let (main_target, main, strict_main) = MAINS[main_choice];
    let folder = main.rfind('/').map_or("", |at| &main[..=at]);
    let rel_base = if extra & 8 != 0 { STRICT_REL } else { REL };
    let (place_name, place_rel) = PLACES[place];
    let mark_part = match place_name {
        "" => main.to_owned(),
        absolute if absolute.starts_with('!') => absolute[1..].to_owned(),
        relative => format!("{folder}{relative}"),
    };
    let lying = flags & 64 != 0;
    let mut must_offer = None;
    if main_target.is_some_and(|t| t.trim_start_matches('/') != main) {
        must_offer = Some("the officeDocument target is missing");
    }
    let mut doc_rels: Vec<(String, String)> = Vec::new();
    if let Some((kind, target)) = place_rel {
        doc_rels.push((format!("{rel_base}{kind}"), target.to_owned()));
        // a leading `/` or a backslash in the zip name, not in the target
        if (17..=18).contains(&place) {
            must_offer = Some("a relationship's target is missing (zip name differs)");
        }
    }
    doc_rels.push((format!("{rel_base}styles"), "styles.xml".to_owned()));
    if lying {
        doc_rels.push((format!("{rel_base}header"), "header9.xml".to_owned()));
        must_offer = Some("a relationship's target is missing");
    }
    if package & 1 != 0 {
        doc_rels.push((format!("{rel_base}aFChunk"), "chunk.htm".to_owned()));
        must_offer = Some("an aFChunk relationship");
    }
    let svg_name = format!("{folder}media/logo.svg");
    if svg > 0 {
        doc_rels.push((format!("{rel_base}image"), "media/logo.svg".to_owned()));
    }

    // the parts, by zip name
    let clean = wrap(0, "", "", "p", "");
    let mut parts: Vec<(String, String)> = Vec::new();
    // one input in eight: a part over 1 MiB costs far more to build and read than the rest
    let oversize = flags & 128 != 0 && over & 0x30 == 0x30;
    let pad = |which: u8| {
        if oversize && (over >> 6) == which {
            padding(usize::from(over & 15) * 997)
        } else {
            String::new()
        }
    };
    if let Some(target) = main_target {
        let kind = if strict_main { STRICT_REL } else { REL };
        let entries = [(format!("{kind}officeDocument"), target.to_owned())];
        parts.push(("_rels/.rels".to_owned(), rels(&entries, &pad(0))));
    }
    parts.push((rels_of(main), rels(&doc_rels, &pad(1))));
    if flags & 16 == 0 {
        let mut ct = String::from(
            "<?xml version=\"1.0\"?><Types \
             xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
             <Default Extension=\"rels\" \
             ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
        );
        if !mark_part.ends_with(".xml") {
            let _ = write!(
                ct,
                "<Override PartName=\"/{mark_part}\" ContentType=\"{HEADER_TYPE}\"/>"
            );
        }
        if lying {
            ct.push_str("<Override PartName=\"/word/missing.xml\" ContentType=\"image/png\"/>");
            must_offer = Some("a part the content types name is missing");
        }
        if svg > 0 {
            let mut kind = String::from("image/svg+xml");
            if package & 16 != 0 {
                kind.push_str("; charset=utf-8");
            }
            if package & 32 != 0 {
                kind = kind.to_uppercase();
            }
            if package & 8 != 0 {
                let _ = write!(
                    ct,
                    "<Override PartName=\"/{svg_name}\" ContentType=\"{kind}\"/>"
                );
            } else {
                let _ = write!(ct, "<Default Extension=\"svg\" ContentType=\"{kind}\"/>");
            }
        }
        ct.push_str(&pad(2));
        ct.push_str("</Types>");
        parts.push(("[Content_Types].xml".to_owned(), ct));
    } else if mark_part != "[Content_Types].xml" {
        must_offer = Some("no content types");
    }
    if mark_part != main {
        parts.push((main.to_owned(), clean.clone()));
    }
    let mut marked = text;
    if oversize && (over >> 6) == 3 {
        marked = marked.replacen("<w:body>", &format!("<w:body>{}", padding(0)), 1);
    }
    // a mark in [Content_Types].xml replaces it
    parts.retain(|(n, _)| *n != mark_part);
    let mark_at = if extra & 4 != 0 { 0 } else { parts.len() };
    parts.insert(mark_at, (mark_part.clone(), marked));
    parts.push((
        format!("{folder}styles.xml"),
        clean.replace("w:document", "w:styles"),
    ));
    if extra & 2 != 0 {
        parts.push(("docProps/app.xml".to_owned(), "<Properties/>".to_owned()));
        parts.push((
            "word/settings.xml".to_owned(),
            clean.replace("w:document", "w:settings"),
        ));
    }
    if package & 1 != 0 {
        parts.push((
            format!("{folder}chunk.htm"),
            "<html><ins>x</ins></html>".to_owned(),
        ));
    }
    let dup = (package & 2 != 0).then_some((mark_at, package & 4 != 0));
    if dup.is_some() {
        must_offer = Some("two entries of one name");
    }
    let svg_at = parts.len();
    if svg > 0 {
        parts.push((svg_name.clone(), SVGS[svg - 1].to_owned()));
    }

    let deflate = flags & 32 != 0 || oversize;
    let mut bytes = write_zip(&parts, deflate, dup)?;
    let mut svg_twin = None;
    if svg > 0 {
        parts[svg_at].1 = PLAIN_SVG.to_owned();
        svg_twin = Some(write_zip(&parts, deflate, dup)?);
    }

    let len = bytes.len();
    let at = usize::from(pos) * len / 65536;
    let broken = match corrupt % 8 {
        4 => {
            bytes.truncate(at);
            true
        }
        5 => {
            bytes[at] ^= 0xFF;
            true
        }
        6 => {
            bytes.extend_from_slice(&[0x50, 0x4B, 0x05, 0x06, 0, 0, 0, 0]);
            true
        }
        7 => {
            let mut prefixed = vec![b'Z'; usize::from(pos % 512)];
            prefixed.extend_from_slice(&bytes);
            bytes = prefixed;
            true
        }
        _ => false,
    };
    let spec = format!(
        "mark={name:?}{} ns={} prefix={prefix:?} on_root={on_root} charref={} place={mark_part:?} \
         wrap={wrap_choice} main={main:?} via={main_target:?} lying={lying} oversize={} \
         corrupt={} svg={svg} package={package:#x} content_types={} tail={}B",
        if mark.is_empty() { " (none)" } else { "" },
        if strict { "strict" } else { "transitional" },
        flags & 8 != 0,
        if oversize { over >> 6 } else { 9 },
        corrupt % 8,
        flags & 16 == 0,
        tail.len(),
    );
    Some(Docx {
        zip: bytes,
        spec,
        must_offer: must_offer.filter(|_| !broken),
        svg_twin: svg_twin.filter(|_| !broken),
    })
}

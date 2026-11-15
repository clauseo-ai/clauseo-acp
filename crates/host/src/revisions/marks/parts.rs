//! Which parts of a Word file the marks scan reads: the parts stetdoc reads tracked changes from,
//! found the way stetdoc finds them. The main document is the package's `officeDocument`
//! relationship (`_rels/.rels`; `word/document.xml` when it names none), and its own
//! relationships (`word/_rels/document.xml.rels`) name its headers, footers, footnotes, endnotes
//! and comments. A relationship to outside the package (`TargetMode="External"`) is not a part.
//!
//! The relationship parts are matched as bytes, never parsed as XML, like the rest of the scan:
//! each `<Relationship` element's `Type`, `Target` and `TargetMode` attributes are read, with the
//! XML entities in a value decoded, and a target is resolved against its source part's folder.
//! The read is one pass: after each element it goes on from the element's end, so a hostile part
//! with `<Relationship` repeated inside one long value costs its length, not its length squared.
//!
//! A target may name any part inside the zip, `/[Content_Types].xml` or a relationships part
//! among them: the scan then reads that part as XML too, within the same caps. It never leaves
//! the zip, so such a target costs a read and nothing more.

/// Where the main document is when the package names none.
const MAIN_FALLBACK: &str = "word/document.xml";

/// The end of the relationship type that names the main document.
const MAIN_TYPE: &str = "/officeDocument";

/// The ends of the relationship types of the parts besides the main document that stetdoc reads
/// tracked changes from (0.9.3 calls them the `headers`, `notes` and `comments` stories).
const STORY_TYPES: &[&str] = &["/header", "/footer", "/footnotes", "/endnotes", "/comments"];

/// The most story parts one file's scan reads: Word writes at most six headers and footers a
/// section. A file naming more may have changes in those it does not read.
pub(super) const MAX_PARTS: usize = 1_024;

/// One `<Relationship>`: its type, its target and whether that target is outside the package.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Relationship {
    pub(super) kind: String,
    pub(super) target: String,
    pub(super) external: bool,
}

/// The main document's name, from the package's relationships `rels` (empty: no such part).
pub(super) fn main_part(rels: &[u8]) -> String {
    relationships(rels)
        .find(|rel| !rel.external && rel.kind.ends_with(MAIN_TYPE))
        .and_then(|rel| resolve("", &rel.target))
        .unwrap_or_else(|| MAIN_FALLBACK.to_owned())
}

/// The name of the relationships part of the part `name`: `word/document.xml`'s is
/// `word/_rels/document.xml.rels`.
pub(super) fn rels_of(name: &str) -> String {
    let (folder, file) = split(name);
    format!("{folder}_rels/{file}.rels")
}

/// The story parts the main document `main`'s relationships `rels` name, in their order, each
/// once, `main` itself not among them; at most one past [`MAX_PARTS`], which tells the caller
/// there were more than it reads.
pub(super) fn story_parts(main: &str, rels: &[u8]) -> Vec<String> {
    let (folder, _) = split(main);
    let mut parts: Vec<String> = Vec::new();
    for rel in relationships(rels) {
        if rel.external || !STORY_TYPES.iter().any(|end| rel.kind.ends_with(end)) {
            continue;
        }
        let Some(name) = resolve(folder, &rel.target) else {
            continue;
        };
        if name != main && !parts.contains(&name) {
            parts.push(name);
        }
        if parts.len() > MAX_PARTS {
            break;
        }
    }
    parts
}

/// The story parts by the names Word gives them, among the zip's entry `names` in their order,
/// at most one past [`MAX_PARTS`] as [`story_parts`] gives them, for a main document whose
/// relationships the scan could not read (too large, or not UTF-8): `word/header*.xml`,
/// `word/footer*.xml`, `word/footnotes.xml`, `word/endnotes.xml` and `word/comments.xml`.
pub(super) fn usual_story_parts<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for name in names {
        let Some(file) = name.strip_prefix("word/") else {
            continue;
        };
        let numbered = ["header", "footer"]
            .iter()
            .any(|start| file.starts_with(start))
            && std::path::Path::new(file)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
            && !file.contains('/');
        let one = matches!(file, "footnotes.xml" | "endnotes.xml" | "comments.xml");
        if (numbered || one) && !parts.iter().any(|part| part == name) {
            parts.push(name.to_owned());
        }
        if parts.len() > MAX_PARTS {
            break;
        }
    }
    parts
}

/// A part name's folder, with its trailing `/` (`""` at the top), and its file name.
fn split(name: &str) -> (&str, &str) {
    match name.rfind('/') {
        Some(at) => name.split_at(at + 1),
        None => ("", name),
    }
}

/// `target` resolved against the folder `folder` (`""` or ending in `/`) as a zip entry name,
/// without a leading `/`; `None` for a target that climbs out of the package or names no file.
fn resolve(folder: &str, target: &str) -> Option<String> {
    let joined = match target.strip_prefix('/') {
        Some(absolute) => absolute.to_owned(),
        None => format!("{folder}{target}"),
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    if segments.is_empty() || joined.ends_with('/') {
        return None;
    }
    Some(segments.join("/"))
}

/// Every `<Relationship` element of `xml` as a [`Relationship`]; one without a `Type` or a
/// `Target` is left out.
pub(super) fn relationships(xml: &[u8]) -> impl Iterator<Item = Relationship> + '_ {
    const OPEN: &[u8] = b"<Relationship";
    let finder = memchr::memmem::Finder::new(OPEN);
    let mut rest = xml;
    std::iter::from_fn(move || {
        loop {
            let at = finder.find(rest)?;
            let after = rest.get(at + OPEN.len()..)?;
            // the element ends at its first `>` (`/>` included); the next search starts there, so
            // no byte is searched twice
            let end = memchr::memchr(b'>', after);
            rest = end.and_then(|end| after.get(end + 1..)).unwrap_or_default();
            // `<Relationships`, the root, is another element
            if !after.first().is_some_and(u8::is_ascii_whitespace) {
                continue;
            }
            if let Some(rel) = relationship(after.get(..end?)?) {
                return Some(rel);
            }
        }
    })
}

/// The relationship whose tag after `<Relationship` is `tag`; `None` without a `Type` or a
/// `Target`. Only those two values are decoded; every other attribute is passed over.
fn relationship(tag: &[u8]) -> Option<Relationship> {
    let mut kind = None;
    let mut target = None;
    let mut external = false;
    for (name, value) in attributes(tag) {
        match name {
            b"Type" => kind = Some(text(value)),
            b"Target" => target = Some(text(value)),
            b"TargetMode" => external = value.eq_ignore_ascii_case(b"External"),
            _ => {}
        }
    }
    Some(Relationship {
        kind: kind?,
        target: target?,
        external,
    })
}

/// The `name="value"` (or `name='value'`) pairs of an element's tag after its name, in one pass,
/// until one does not read.
fn attributes(mut tag: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    std::iter::from_fn(move || {
        let eq = memchr::memchr(b'=', tag)?;
        let name = tag.get(..eq)?.trim_ascii();
        let value = tag.get(eq + 1..)?.trim_ascii_start();
        let (&quote, value) = value.split_first()?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        let close = memchr::memchr(quote, value)?;
        tag = value.get(close + 1..)?;
        Some((name, value.get(..close)?))
    })
}

/// An attribute's value as text, its five predefined XML entities decoded (one copy when it has
/// none).
fn text(value: &[u8]) -> String {
    let value = String::from_utf8_lossy(value);
    if !value.contains('&') {
        return value.into_owned();
    }
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        Relationship, main_part, relationships, rels_of, resolve, story_parts, usual_story_parts,
    };
    use crate::clock::{Clock as _, SystemClock};

    const DOC_RELS: &str = concat!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://x/rel">"#,
        r#"<Relationship Id="rId1" Type="http://x/officeDocument/2006/relationships/styles" "#,
        r#"Target="styles.xml"/>"#,
        r#"<Relationship Target='header1.xml' Id="rId2" "#,
        "\n  Type=\"http://x/officeDocument/2006/relationships/header\"/>",
        r#"<Relationship Id="rId3" Type="http://x/relationships/footer" Target="/word/f&amp;1.xml"/>"#,
        r#"<Relationship Id="rId4" Type="http://x/relationships/footnotes" Target="notes/fn.xml"/>"#,
        r#"<Relationship Id="rId5" Type="http://x/relationships/endnotes" Target="../en.xml"/>"#,
        r#"<Relationship Id="rId6" Type="http://x/relationships/comments" Target="comments.xml"/>"#,
        r#"<Relationship Id="rId7" Type="http://x/relationships/header" Target="header1.xml"/>"#,
        r#"<Relationship Id="rId8" Type="http://x/relationships/header" Target="http://a/h.xml" "#,
        r#"TargetMode="External"/>"#,
        r#"<Relationship Id="rId9" Type="http://x/relationships/footer" Target="../../out.xml"/>"#,
        r#"</Relationships>"#,
    );

    #[test]
    fn the_story_parts_are_the_ones_the_document_s_relationships_name() {
        assert_eq!(
            story_parts("word/document.xml", DOC_RELS.as_bytes()),
            [
                "word/header1.xml",
                "word/f&1.xml",
                "word/notes/fn.xml",
                "en.xml",
                "word/comments.xml",
            ]
        );
        assert_eq!(rels_of("word/document.xml"), "word/_rels/document.xml.rels");
        assert_eq!(rels_of("doc.xml"), "_rels/doc.xml.rels");
    }

    #[test]
    fn the_main_document_is_the_package_s_office_document() {
        let rels = concat!(
            r#"<Relationships><Relationship Id="rId2" Type="http://x/relationships/core-properties" "#,
            r#"Target="docProps/core.xml"/><Relationship Id="rId1" "#,
            r#"Type="http://x/officeDocument/2006/relationships/officeDocument" "#,
            r#"Target="/word/document2.xml"/></Relationships>"#,
        );
        assert_eq!(main_part(rels.as_bytes()), "word/document2.xml");
        assert_eq!(main_part(b"<Relationships/>"), "word/document.xml");
        assert_eq!(main_part(b""), "word/document.xml");
    }

    #[test]
    fn a_relationship_reads_in_either_quote_and_a_broken_one_not_at_all() {
        let xml = br#"<Relationship Type='t' Target="a.xml" TargetMode="external"/><Relationship Type="t"/><Relationship Type="t" Target=broken/>"#;
        assert_eq!(
            relationships(xml).collect::<Vec<_>>(),
            [Relationship {
                kind: "t".to_owned(),
                target: "a.xml".to_owned(),
                external: true,
            }]
        );
        assert_eq!(
            resolve("word/", "./a/../b.xml").as_deref(),
            Some("word/b.xml")
        );
        assert_eq!(resolve("word/", "../../x.xml"), None);
        assert_eq!(resolve("word/", "a/"), None);
        assert_eq!(resolve("", "/"), None);
    }

    /// `<Relationship ` repeated inside one value, `size` bytes of it: each one is the start of
    /// another element to a scan that reads ahead from every match.
    fn hostile(size: usize) -> String {
        let inside = "<Relationship ".repeat(size / "<Relationship ".len());
        format!(
            r#"<Relationships><Relationship Id="{inside}" Type="t/header" Target="h.xml"/></Relationships>"#
        )
    }

    /// The shape that took 15 s at 256 KiB and over two minutes at 1 MiB reads in one pass: well
    /// under a second, even in a debug build.
    #[test]
    fn a_relationships_part_with_elements_hidden_in_a_value_reads_in_one_pass() {
        for size in [256 * 1024, 1024 * 1024] {
            let xml = hostile(size);
            let start = SystemClock.now().mono;
            let found: Vec<Relationship> = relationships(xml.as_bytes()).collect();
            let took = SystemClock.now().mono.saturating_sub(start);
            assert_eq!(found.len(), 1, "{size}");
            assert_eq!(found[0].target, "h.xml");
            assert!(took < Duration::from_secs(1), "{size} bytes took {took:?}");
        }
    }

    #[test]
    fn the_usual_story_parts_are_word_s_names_for_them() {
        let names = [
            "[Content_Types].xml",
            "word/document.xml",
            "word/header1.xml",
            "word/footer12.xml",
            "word/footnotes.xml",
            "word/endnotes.xml",
            "word/comments.xml",
            "word/styles.xml",
            "word/media/header.png",
            "word/headers/a.xml",
            "header1.xml",
            "word/commentsExtended.xml",
        ];
        assert_eq!(
            usual_story_parts(names.into_iter()),
            [
                "word/header1.xml",
                "word/footer12.xml",
                "word/footnotes.xml",
                "word/endnotes.xml",
                "word/comments.xml",
            ]
        );
    }
}
